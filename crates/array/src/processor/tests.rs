use std::collections::HashSet;

use num_complex::Complex;
use sdrmm_dsp::array::manifold::Geometry;
use sdrmm_wire::form::audit::audit;
use sdrmm_wire::{
    ArrayElement, ArrayGeometry, NodeBody, PatchCatalog, PortDirection, ProcessorNode,
    ProcessorParams, STEER_PORT, SpatialSpectrumOwned, Winding, processor_type,
};

use super::bench::{Bench, Sink, block};
use super::*;
use crate::beamformer::BeamformerParams;
use crate::df::DfParams;
use crate::passive_radar::PassiveRadarParams;
use crate::polarimeter::PolarimeterParams;
use crate::stitch::StitchParams;

fn created(bench: &Bench, params: &Params) -> Result<(), ChannelError> {
    create_processor(&bench.ctx(), params).map(|_| ())
}

fn refusal(result: Result<(), ChannelError>) -> String {
    match result {
        Err(error) => error.to_string(),
        Ok(()) => String::from("built"),
    }
}

fn built_in_descriptor(type_id: &str) -> Option<Descriptor> {
    crate::descriptor(type_id)
}

fn built_in() -> Vec<Descriptor> {
    crate::builtin::all()
}

fn node(type_id: &str) -> NodeBody {
    NodeBody::Processor(ProcessorNode {
        processor_type: type_id.to_owned(),
        settings: serde_json::Value::Null,
    })
}

fn defaults(descriptor: &dyn ProcessorDescriptor) -> Params {
    descriptor
        .parse(&ProcessorParams::bare(descriptor.type_id()))
        .expect("defaults parse")
}

#[test]
fn processor_descriptors_are_unique_and_named() {
    let all = built_in();
    let catalog = PatchCatalog::build();
    let ids: HashSet<&str> = all.iter().map(|descriptor| descriptor.type_id()).collect();
    assert_eq!(ids.len(), all.len());
    for descriptor in &all {
        let type_id = descriptor.type_id();
        assert!(!descriptor.name().is_empty(), "{type_id}");
        assert!(descriptor.kind().min_lanes >= 2, "{type_id}");
        assert!(descriptor.kind().max_lanes as usize <= MAX_LANES);
        assert!(descriptor.lane_ports().len() <= MAX_LANE_PORTS);
        assert_eq!(
            crate::descriptor(type_id).map(|found| found.name().to_owned()),
            Some(descriptor.name().to_owned())
        );
        let listed = catalog
            .processors
            .iter()
            .find(|kind| kind.type_id == type_id)
            .expect("the catalog lists every processor");
        assert_eq!(listed.name, descriptor.name());
    }
    assert!(crate::descriptor("combiner").is_none());
}

#[test]
fn descriptor_lane_ports_match_the_wire_port_table() {
    for descriptor in built_in() {
        let type_id = descriptor.type_id();
        let body = node(type_id);
        assert_eq!(descriptor.lane_ports(), body.lane_outputs(), "{type_id}");
        let steered = body
            .ports(None)
            .iter()
            .any(|port| port.name == STEER_PORT && port.direction == PortDirection::In);
        assert_eq!(descriptor.steer_port().is_some(), steered, "{type_id}");
    }
}

#[test]
fn descriptor_forms_pass_the_audit_and_the_catalog_publishes_them() {
    for descriptor in built_in() {
        let type_id = descriptor.type_id();
        let form = &descriptor.kind().settings;
        assert_eq!(
            processor_type(type_id).map(|kind| kind.settings),
            Some(form.clone()),
            "{type_id}"
        );
        if form.is_empty() {
            continue;
        }
        audit(type_id, form, &descriptor.kind().defaults, &|settings| {
            let wire = ProcessorParams {
                type_id: type_id.to_owned(),
                settings,
            };
            descriptor
                .parse(&wire)
                .and_then(|params| params.to_wire())
                .unwrap_or_else(|error| panic!("{type_id} settings refused: {error}"))
                .settings
        });
    }
}

#[test]
fn descriptors_agree_with_the_facets_they_publish() {
    for descriptor in built_in() {
        let type_id = descriptor.type_id();
        let facets = &descriptor.kind().facets;
        let clear = ProcessorAction::ClearTracks;
        assert_eq!(
            descriptor.takes(clear),
            facets.actions.iter().any(|spec| spec.action == clear),
            "{type_id}"
        );
        let params = defaults(descriptor.as_ref());
        assert_eq!(params.problem(), None, "{type_id} defaults are valid");
        let spread = descriptor.tuning(&params) == TuningNeed::Spread;
        assert_eq!(
            facets.tuning == Some(ArrayTuningMode::Spread),
            spread,
            "{type_id}"
        );
    }
}

#[test]
fn descriptors_read_their_own_settings() {
    let df = Params::new(DfParams::default());
    let descriptor = built_in_descriptor("df").expect("df");
    assert_eq!(descriptor.band(&df), Some((0.0, 20_000.0)));
    assert_eq!(descriptor.needs(&df), ProcessorNeeds::ALL);
    assert!(descriptor.in_place(&df, &df));
    let narrower = Params::new(DfParams {
        bandwidth_hz: 10_000.0,
        ..DfParams::default()
    });
    assert!(!descriptor.in_place(&df, &narrower));
    let stitch = Params::new(StitchParams::default());
    assert!(!descriptor.in_place(&df, &stitch));
    let radar = built_in_descriptor("passive_radar").expect("radar");
    let bench = Bench::together("descriptors", 3, 2_048_000.0, 4_096);
    assert_eq!(radar.execution(&stitch, &bench.ctx()), Execution::Dedicated);
    assert_eq!(radar.needs(&stitch), ProcessorNeeds::TIME);
}

#[test]
fn create_processor_refuses_too_few_lanes() {
    let one = Bench::together("one", 1, 2_048_000.0, 4_096);
    let many = Bench::together("many", MAX_LANES + 1, 2_048_000.0, 4_096);
    for params in [
        Params::new(StitchParams::default()),
        Params::new(DfParams::default()),
    ] {
        assert_eq!(refusal(created(&one, &params)), "Too few elements");
        assert_eq!(refusal(created(&many, &params)), "Too many elements");
    }
    let wrong = Params::new(DfParams {
        max_peaks: 0,
        ..DfParams::default()
    });
    let two = Bench::together("two", 2, 2_048_000.0, 4_096);
    assert_eq!(refusal(created(&two, &wrong)), "Peaks out of range");
}

#[test]
fn descriptors_declare_what_the_engine_offers_them() {
    let radar = built_in_descriptor("passive_radar").expect("radar");
    assert!(radar.takes(ProcessorAction::ClearTracks));
    let off = Params::new(PassiveRadarParams {
        gpu: GpuUse::Off,
        ..PassiveRadarParams::default()
    });
    assert_eq!(radar.gpu(&off), GpuUse::Off);
    let auto = Params::new(PassiveRadarParams::default());
    assert_eq!(radar.gpu(&auto), GpuUse::Auto);
    let df = built_in_descriptor("df").expect("df");
    assert!(!df.takes(ProcessorAction::ClearTracks));
    assert_eq!(df.gpu(&Params::new(DfParams::default())), GpuUse::Off);
    assert!(
        built_in()
            .iter()
            .all(|descriptor| descriptor.event_bytes() == 0)
    );
}

#[test]
fn dedicated_processors_are_not_created_here() {
    let bench = Bench::together("radar", 3, 2_048_000.0, 4_096);
    let radar = Params::new(PassiveRadarParams::default());
    let Err(ChannelError::Unsupported(text)) = created(&bench, &radar) else {
        panic!("a radar is never built here");
    };
    assert_eq!(text, "passive_radar is built by the engine");
}

#[test]
fn lane_writer_truncates_and_counts_overflow() {
    let mut sink = Sink::new("stitch", &[4]);
    let samples = [Complex::new(1.0, 0.0); 3];
    sink.run(|out| {
        let mut lane = out.lane(0).expect("port 0");
        lane.extend(&samples);
        assert_eq!(lane.room(), 1);
        lane.push(Complex::new(2.0, 0.0));
        lane.push(Complex::new(3.0, 0.0));
        lane.extend(&samples);
        assert_eq!(lane.room(), 0);
        assert!(out.lane(1).is_none());
        out.skip_lane(0, 7);
    });
    let lane = &sink.lanes[0];
    assert_eq!(lane.samples().len(), 4);
    assert_eq!(lane.samples()[3], Complex::new(2.0, 0.0));
    assert_eq!(lane.overflowed(), 4);
    assert_eq!(lane.skipped(), 7);
    assert_eq!(lane.capacity(), 4);
}

#[test]
fn output_slots_report_an_empty_pool() {
    let mut events = Vec::new();
    let mut out = ProcessorOutput::new(OutputSlots {
        report: None,
        surface: None,
        bearings: &mut events,
        lanes: &mut [],
    });
    assert!(out.report().is_none());
    out.publish_report();
    assert!(out.surface().is_none());
    assert!(out.bearing().is_none());
    out.publish_bearing();
    let tally = out.tally();
    assert!(!tally.report && !tally.surface);
    assert_eq!(tally.bearings, 0);
    assert_eq!(tally.dropped_reports, 1);
    assert_eq!(tally.dropped_surfaces, 1);
    assert_eq!(tally.dropped_bearings, 1);
}

#[test]
fn a_missing_surface_slot_counts_as_a_surface_and_not_as_a_report() {
    let mut sink = Sink::new("spatial_spectrum", &[]);
    sink.surface = Some(SurfaceFrame::SpatialSpectrum(
        SpatialSpectrumOwned::default(),
    ));
    let tally = sink.run(|out| {
        assert!(out.surface().is_some());
        out.publish_surface();
        assert!(out.surface().is_none());
        assert!(out.report().is_some());
        out.publish_report();
    });
    assert!(tally.report && tally.surface);
    assert_eq!(tally.dropped_surfaces, 1);
    assert_eq!(tally.dropped_reports, 0);
}

#[test]
fn output_slots_hand_out_one_report_and_bounded_events() {
    let mut sink = Sink::new("stitch", &[]);
    let tally = sink.run(|out| {
        assert!(out.report().is_some());
        out.publish_report();
        assert!(out.report().is_none());
        for _ in 0..3 {
            if out.bearing().is_some() {
                out.publish_bearing();
            }
        }
        out.steer(Steer {
            relative_deg: 42.0,
            ..Steer::default()
        });
    });
    assert!(tally.report);
    assert_eq!(tally.dropped_reports, 1);
    assert_eq!(tally.bearings, 2);
    assert_eq!(tally.dropped_bearings, 1);
    assert_eq!(tally.steer.map(|steer| steer.relative_deg), Some(42.0));
}

fn close(a: &[f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

fn matches_wire(geometry: &ArrayGeometry, lanes: usize) {
    let built: Geometry = geometry_of(geometry, lanes).expect("geometry");
    let expected = geometry.positions(lanes).expect("wire positions");
    assert_eq!(built.len(), lanes);
    for (position, want) in built.positions().iter().zip(&expected) {
        assert!(
            close(want, [position.x, position.y, position.z]),
            "{geometry:?}: {position:?} vs {want:?}"
        );
    }
}

#[test]
fn geometry_of_converts_every_shape() {
    matches_wire(
        &ArrayGeometry::Uca {
            radius_m: 0.35,
            first_deg: 15.0,
            winding: Winding::CounterClockwise,
        },
        5,
    );
    matches_wire(&ArrayGeometry::default(), 4);
    matches_wire(
        &ArrayGeometry::Ula {
            spacing_m: 0.4,
            axis_deg: 30.0,
        },
        4,
    );
    let explicit = ArrayGeometry::Explicit {
        positions: vec![
            ArrayElement {
                x_m: 0.0,
                y_m: 0.0,
                z_m: 0.0,
            },
            ArrayElement {
                x_m: 0.5,
                y_m: 0.1,
                z_m: 0.2,
            },
            ArrayElement {
                x_m: -0.3,
                y_m: 0.6,
                z_m: 0.0,
            },
        ],
    };
    matches_wire(&explicit, 3);
    let problem = |geometry: &ArrayGeometry, lanes: usize| {
        geometry_of(geometry, lanes)
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
    assert_eq!(
        problem(&explicit, 4),
        Err("Needs array geometry".to_owned())
    );
    assert_eq!(
        problem(&ArrayGeometry::default(), 1),
        Err("Too few elements".to_owned())
    );
    let flat = ArrayGeometry::Ula {
        spacing_m: 0.0,
        axis_deg: 0.0,
    };
    assert_eq!(problem(&flat, 3), Err("Needs array geometry".to_owned()));
}

#[test]
fn tuning_checks_name_the_layout_they_need() {
    let together = Bench::together("together", 3, 1e6, 1_024);
    let spread = Bench::spread("spread", 1e6, &[-4e5, 0.0, 4e5], 1_024);
    assert!(!lanes_spread(&together.ctx()));
    assert!(lanes_spread(&spread.ctx()));
    assert!(check_tuning(TuningNeed::Together, &together.ctx()).is_ok());
    assert!(check_tuning(TuningNeed::Any, &spread.ctx()).is_ok());
    assert_eq!(
        check_tuning(TuningNeed::Together, &spread.ctx()).map_err(|e| e.to_string()),
        Err("Needs lanes tuned together".to_owned())
    );
    assert_eq!(
        check_tuning(TuningNeed::Spread, &together.ctx()).map_err(|e| e.to_string()),
        Err("Needs spread tuning".to_owned())
    );
}

#[test]
fn correction_response_is_relative_to_the_reference_lane() {
    let bins = 64;
    let rate = 64_000.0;
    let common = |k: usize| Complex::from_polar(1.0f32, -0.3 * k as f32);
    let spectra = vec![
        (0..bins).map(common).collect::<Vec<_>>(),
        (0..bins)
            .map(|k| common(k) * Complex::from_polar(0.5, 1.0))
            .collect(),
    ];
    let view = CorrectionView::new(3, rate, &spectra);
    let lane = view.response(1, 5_500.0);
    assert!((lane.norm() - 0.5).abs() < 1e-5, "{lane}");
    assert!((lane.arg() - 1.0).abs() < 1e-5, "{lane}");
    assert!((view.response(0, -12_000.0) - Complex::new(1.0, 0.0)).norm() < 1e-5);
    assert_eq!(view.response(2, 0.0), Complex::new(1.0, 0.0));
    assert_eq!(
        CorrectionView::identity().response(1, 0.0),
        Complex::new(1.0, 0.0)
    );
}

#[test]
fn stamp_at_writes_rfc3339_in_place() {
    let mut at = String::with_capacity(40);
    let pointer = at.as_ptr();
    stamp_at(&mut at, 1_790_000_000_123_456_789);
    assert_eq!(at, "2026-09-21T14:13:20.123Z");
    assert_eq!(at.as_ptr(), pointer);
    stamp_at(&mut at, 951_782_400_000_000_000);
    assert_eq!(at, "2000-02-29T00:00:00.000Z");
}

#[test]
fn a_block_is_as_long_as_its_shortest_lane() {
    let long = [Complex::new(0.0, 0.0); 8];
    let short = [Complex::new(0.0, 0.0); 5];
    let lanes: [&[Complex<f32>]; 2] = [&long, &short];
    assert_eq!(block(&lanes, 0).len(), 5);
    assert!(block(&[], 0).is_empty());
}

#[cfg(feature = "probe")]
mod probe_tests {
    use super::super::probe::{PROBE_PORTS, ProbeParams, take_probe_log};
    use super::*;

    fn probe(ports: u8) -> Params {
        Params::new(ProbeParams {
            phase: true,
            lane_ports: ports,
            ..ProbeParams::default()
        })
    }

    fn lanes(count: usize, len: usize) -> Vec<Vec<Complex<f32>>> {
        (0..count)
            .map(|lane| vec![Complex::new(lane as f32 + 1.0, 0.0); len])
            .collect()
    }

    #[test]
    fn probe_processor_records_blocks_in_element_order() {
        let bench = Bench::together("probe-order", 3, 48_000.0, 256);
        let mut processor = create_processor(&bench.ctx(), &probe(2)).expect("probe");
        let mut log = take_probe_log("probe-order").expect("log");
        let owned = lanes(3, 100);
        let views: Vec<&[Complex<f32>]> = owned.iter().map(Vec::as_slice).collect();
        let mut sink = Sink::new("stitch", &[256, 256]);
        sink.run(|out| processor.process(&block(&views, 7), out));
        processor.reset(ResetCause::Gap);
        sink.run(|out| processor.process(&block(&views, 8), out));
        let blocks = log.drain();
        assert_eq!(blocks.len(), 2);
        let first = blocks[0];
        assert_eq!(first.lanes, 3);
        assert_eq!(first.len, 100);
        assert_eq!(first.seq, 0);
        assert_eq!(
            first.first[..3],
            [
                Complex::new(1.0, 0.0),
                Complex::new(2.0, 0.0),
                Complex::new(3.0, 0.0)
            ]
        );
        assert!((first.power[2] - 9.0).abs() < 1e-6);
        assert_eq!(blocks[1].seq, 1);
        assert_eq!(blocks[1].last_reset, Some(ResetCause::Gap));
        assert_eq!(sink.lanes[0].samples(), views[0]);
        assert_eq!(sink.lanes[1].samples(), views[1]);
    }

    #[test]
    fn a_lane_count_mismatch_is_counted_not_silent() {
        let bench = Bench::together("probe-mismatch", 3, 48_000.0, 256);
        let mut processor = create_processor(&bench.ctx(), &probe(1)).expect("probe");
        let mut log = take_probe_log("probe-mismatch").expect("log");
        let owned = lanes(2, 64);
        let views: Vec<&[Complex<f32>]> = owned.iter().map(Vec::as_slice).collect();
        let mut sink = Sink::new("stitch", &[256]);
        let tally = sink.run(|out| processor.process(&block(&views, 0), out));
        assert_eq!(processor.faults().lane_mismatch, 1);
        assert_eq!(tally, OutputTally::default());
        assert!(sink.lanes[0].samples().is_empty());
        assert_eq!(sink.lanes[0].skipped(), 64);
        assert!(log.pop().is_none());
    }

    #[test]
    fn probe_descriptor_follows_its_settings() {
        let descriptor = built_in_descriptor("probe").expect("probe");
        let bench = Bench::together("probe-descriptor", 2, 48_000.0, 512);
        let params = probe(1);
        assert_eq!(descriptor.lane_ports(), PROBE_PORTS);
        assert_eq!(
            descriptor.needs(&params),
            ProcessorNeeds {
                phase: true,
                ..ProcessorNeeds::default()
            }
        );
        assert_eq!(descriptor.tuning(&params), TuningNeed::Together);
        assert_eq!(
            descriptor.lane_format(&params, &bench.ctx(), 0).capacity,
            512
        );
        assert_eq!(descriptor.lane_format(&params, &bench.ctx(), 1).capacity, 0);
        let rebuilt = Params::new(ProbeParams {
            rebuild: 1,
            lane_ports: 1,
            ..ProbeParams::default()
        });
        assert!(descriptor.in_place(&params, &probe(1)));
        assert!(!descriptor.in_place(&params, &rebuilt));
    }
}

#[test]
fn banded_processors_leave_the_aggregator_above_the_inline_limit() {
    let light = Bench::together("light", 2, 1e6, 4_096);
    assert_eq!(banded_execution(&light.ctx()), Execution::Inline);
    let heavy = Bench::together("heavy", 5, 2.4e6, 4_096);
    assert_eq!(
        banded_execution(&heavy.ctx()),
        Execution::Worker { batch: 4_096 }
    );
    for params in [
        Params::new(DfParams::default()),
        Params::new(BeamformerParams::default()),
        Params::new(PolarimeterParams::default()),
    ] {
        let descriptor = built_in_descriptor(params.type_id()).expect("a descriptor");
        assert_eq!(
            descriptor.execution(&params, &light.ctx()),
            Execution::Inline
        );
        assert!(matches!(
            descriptor.execution(&params, &heavy.ctx()),
            Execution::Worker { .. }
        ));
    }
}

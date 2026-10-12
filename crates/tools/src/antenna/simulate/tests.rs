use num_complex::Complex64;
use sdrmm_wire::{
    AntennaDesign, AntennaGeometry, AntennaGround, AntennaMaterial, AntennaModel, AntennaPoint,
    AntennaPolarization, AntennaRequest, AntennaSegment, AntennaSegmentRole, AntennaSimulation,
    AntennaSimulationOutcome, AntennaSweepRequest, YagiParams,
};

use super::super::report;
use super::super::solver::{Medium, solve};
use super::super::structure::Structure;
use super::{Job, simulate};

const ONE_METRE_HZ: f64 = 299_792_458.0;

fn wire(from: (f64, f64, f64), to: (f64, f64, f64)) -> AntennaSegment {
    AntennaSegment {
        label: "Wire".to_owned(),
        role: AntennaSegmentRole::Driven,
        from: AntennaPoint::new(from.0, from.1, from.2),
        to: AntennaPoint::new(to.0, to.1, to.2),
    }
}

fn half_wave(split: bool) -> AntennaGeometry {
    let segments = if split {
        vec![
            wire((-0.25, 0.0, 0.0), (0.0, 0.0, 0.0)),
            wire((0.0, 0.0, 0.0), (0.25, 0.0, 0.0)),
        ]
    } else {
        vec![wire((-0.25, 0.0, 0.0), (0.25, 0.0, 0.0))]
    };
    AntennaGeometry {
        segments,
        feed: AntennaPoint::ORIGIN,
    }
}

fn impedance(geometry: &AntennaGeometry) -> Complex64 {
    let structure = Structure::build(geometry, 0.0, 0.025).expect("a valid structure");
    let medium = Medium {
        frequency_hz: ONE_METRE_HZ,
        radius_m: 1e-5,
        conductivity_s_m: None,
        image: false,
    };
    solve(&structure, &medium).expect("a solution").impedance()
}

fn modelled(geometry: &AntennaGeometry, model: AntennaModel) -> AntennaSimulation {
    let job = Job {
        frequency_hz: ONE_METRE_HZ,
        velocity_factor: 0.95,
        model,
        sweep: None,
        geometry,
        loading_coil: false,
    };
    solved(simulate(&job, None))
}

fn solved(outcome: AntennaSimulationOutcome) -> AntennaSimulation {
    match outcome {
        AntennaSimulationOutcome::Solved(simulation) => *simulation,
        AntennaSimulationOutcome::Unavailable { reason } => panic!("not solved: {reason}"),
    }
}

fn unavailable(outcome: AntennaSimulationOutcome) -> String {
    match outcome {
        AntennaSimulationOutcome::Solved(_) => panic!("expected no solution"),
        AntennaSimulationOutcome::Unavailable { reason } => reason,
    }
}

fn thin(ground: AntennaGround, height_m: f64) -> AntennaModel {
    AntennaModel {
        wire_diameter_mm: 0.02,
        material: AntennaMaterial::Lossless,
        ground,
        height_m,
    }
}

fn request(design: AntennaDesign, model: AntennaModel) -> AntennaRequest {
    AntennaRequest {
        frequency_hz: 14_200_000.0,
        design,
        model,
        ..AntennaRequest::default()
    }
}

#[test]
fn a_thin_half_wave_dipole_has_the_textbook_impedance() {
    let z = impedance(&half_wave(false));
    assert!((72.0..82.0).contains(&z.re), "R {}", z.re);
    assert!((36.0..48.0).contains(&z.im), "X {}", z.im);
}

#[test]
fn feeding_at_a_junction_matches_feeding_inside_a_wire() {
    let whole = impedance(&half_wave(false));
    let split = impedance(&half_wave(true));
    assert!((whole - split).norm() < 1e-6, "{whole} against {split}");
}

#[test]
fn a_free_space_dipole_has_two_dbi_and_a_figure_eight() {
    let simulation = modelled(&half_wave(false), thin(AntennaGround::FreeSpace, 0.0));
    assert!(
        (simulation.gain_dbi - 2.15).abs() < 0.15,
        "{}",
        simulation.gain_dbi
    );
    assert!(simulation.takeoff_deg.abs() < 0.1);
    assert!(simulation.bearing_deg.abs() < 0.1);
    let width = simulation.beamwidth_azimuth_deg.expect("a figure eight");
    assert!((width - 78.0).abs() < 3.0, "{width}");
    assert_eq!(simulation.beamwidth_elevation_deg, None);
    assert_eq!(simulation.polarization, AntennaPolarization::Horizontal);
    assert!(simulation.front_to_back_db.abs() < 0.1);
    assert_eq!(simulation.azimuth.gain_dbi.len(), 180);
    assert_eq!(simulation.elevation.gain_dbi.len(), 180);
}

#[test]
fn a_dipole_half_a_wave_over_perfect_ground_lifts_its_lobe_to_thirty_degrees() {
    let simulation = modelled(&half_wave(false), thin(AntennaGround::Perfect, 0.5));
    assert!(
        (7.0..9.0).contains(&simulation.gain_dbi),
        "{}",
        simulation.gain_dbi
    );
    assert!(
        (simulation.takeoff_deg - 30.0).abs() < 3.0,
        "{}",
        simulation.takeoff_deg
    );
    assert_eq!(simulation.elevation.gain_dbi.len(), 91);
}

#[test]
fn a_dipole_far_above_the_ground_sees_free_space() {
    let free = modelled(&half_wave(false), thin(AntennaGround::FreeSpace, 0.0));
    let high = modelled(&half_wave(false), thin(AntennaGround::Perfect, 20.0));
    let gap = Complex64::new(
        free.impedance.re - high.impedance.re,
        free.impedance.im - high.impedance.im,
    );
    assert!(gap.norm() < 2.0, "{gap}");
}

#[test]
fn real_ground_loses_what_perfect_ground_returns() {
    let perfect = modelled(&half_wave(false), thin(AntennaGround::Perfect, 0.5));
    let average = modelled(&half_wave(false), thin(AntennaGround::Average, 0.5));
    assert!(average.gain_dbi < perfect.gain_dbi);
}

#[test]
fn resistive_metal_costs_efficiency() {
    let model = |material| AntennaModel {
        wire_diameter_mm: 0.5,
        material,
        ground: AntennaGround::FreeSpace,
        height_m: 0.0,
    };
    let lossless = modelled(&half_wave(false), model(AntennaMaterial::Lossless));
    let copper = modelled(&half_wave(false), model(AntennaMaterial::Copper));
    let steel = modelled(&half_wave(false), model(AntennaMaterial::StainlessSteel));
    assert!((lossless.efficiency - 1.0).abs() < 1e-9);
    assert!(copper.efficiency < 1.0 && copper.efficiency > 0.95);
    assert!(steel.efficiency < copper.efficiency);
}

#[test]
fn current_peaks_at_the_centre_of_a_dipole_and_dies_at_the_tips() {
    let simulation = modelled(&half_wave(false), thin(AntennaGround::FreeSpace, 0.0));
    let current: Vec<f64> = simulation
        .wires
        .iter()
        .flat_map(|wire| wire.current.iter().copied())
        .collect();
    let peak = current.iter().copied().fold(0.0, f64::max);
    assert!((peak - 1.0).abs() < 1e-9);
    assert!(current.first().copied().unwrap_or(1.0) < 0.2);
    assert!(current.last().copied().unwrap_or(1.0) < 0.2);
    assert_eq!(simulation.wires.len(), 2);
    let feed = &simulation.wires[simulation.feed_wire as usize];
    assert!(simulation.feed_segment < feed.segments);
}

#[test]
fn a_sweep_covers_the_requested_span() {
    let job = Job {
        frequency_hz: ONE_METRE_HZ,
        velocity_factor: 0.95,
        model: thin(AntennaGround::FreeSpace, 0.0),
        sweep: Some(AntennaSweepRequest {
            start_hz: ONE_METRE_HZ * 0.8,
            stop_hz: ONE_METRE_HZ * 1.2,
            points: 21,
        }),
        geometry: &half_wave(false),
        loading_coil: false,
    };
    let simulation = solved(simulate(&job, None));
    assert_eq!(simulation.sweep.len(), 21);
    assert!((simulation.sweep[0].frequency_hz - ONE_METRE_HZ * 0.8).abs() < 1e-3);
    assert!((simulation.sweep[20].frequency_hz - ONE_METRE_HZ * 1.2).abs() < 1e-3);
    let low = simulation.sweep[0].impedance.im;
    let high = simulation.sweep[20].impedance.im;
    assert!(low < 0.0 && high > 0.0, "{low} {high}");
}

#[test]
fn trimming_finds_the_resonant_factor() {
    let trimmed = report(&AntennaRequest {
        trim: true,
        ..request(AntennaDesign::Dipole, AntennaModel::default())
    })
    .expect("a valid request");
    let factor = solved(trimmed.simulation)
        .resonant_velocity_factor
        .expect("a resonance");
    let resonant = report(&AntennaRequest {
        velocity_factor: factor,
        ..request(AntennaDesign::Dipole, AntennaModel::default())
    })
    .expect("a valid request");
    let reactance = solved(resonant.simulation).impedance.im;
    assert!(reactance.abs() < 2.0, "{reactance}");
}

#[test]
fn untrimmed_reports_skip_the_search() {
    let plain = report(&request(AntennaDesign::Dipole, AntennaModel::default())).expect("valid");
    assert_eq!(solved(plain.simulation).resonant_velocity_factor, None);
}

#[test]
fn a_five_eighths_vertical_gets_the_coil_that_cancels_its_reactance() {
    let report = report(&request(
        AntennaDesign::FiveEighthsVertical,
        AntennaModel::default(),
    ))
    .expect("a valid request");
    let simulation = solved(report.simulation);
    assert!(simulation.impedance.im.abs() < 0.01);
    assert!(simulation.loading_coil_h.expect("a coil") > 0.0);
    assert_eq!(simulation.polarization, AntennaPolarization::Vertical);
}

#[test]
fn an_end_fed_half_wave_is_a_high_impedance() {
    let report = report(&request(
        AntennaDesign::EndFedHalfWave,
        AntennaModel::default(),
    ))
    .expect("a valid request");
    assert!(solved(report.simulation).impedance.re > 1_000.0);
}

#[test]
fn a_yagi_beams_along_its_boom() {
    let report = report(&request(
        AntennaDesign::Yagi(YagiParams::default()),
        AntennaModel {
            ground: AntennaGround::FreeSpace,
            ..AntennaModel::default()
        },
    ))
    .expect("a valid request");
    let simulation = solved(report.simulation);
    assert!(
        simulation.bearing_deg.abs() < 1.0,
        "{}",
        simulation.bearing_deg
    );
    assert!(simulation.gain_dbi > 6.0, "{}", simulation.gain_dbi);
    assert!(simulation.front_to_back_db > 6.0);
}

#[test]
fn an_antenna_below_ground_asks_to_be_raised() {
    let reason = unavailable(
        report(&request(
            AntennaDesign::InvertedV(sdrmm_wire::InvertedVParams::default()),
            AntennaModel {
                height_m: 1.0,
                ..AntennaModel::default()
            },
        ))
        .expect("a valid request")
        .simulation,
    );
    assert!(reason.contains("raise the feedpoint"), "{reason}");
}

#[test]
fn a_wire_thicker_than_the_wavelength_allows_is_not_modelled() {
    let reason = unavailable(
        report(&AntennaRequest {
            frequency_hz: 10_000_000_000.0,
            ..request(AntennaDesign::Dipole, AntennaModel::default())
        })
        .expect("a valid request")
        .simulation,
    );
    assert!(reason.contains("too thick"), "{reason}");
}

#[test]
fn sweeps_outside_the_limits_are_refused() {
    let with = |start_hz: f64, stop_hz: f64, points: u16| AntennaRequest {
        sweep: Some(AntennaSweepRequest {
            start_hz,
            stop_hz,
            points,
        }),
        ..request(AntennaDesign::Dipole, AntennaModel::default())
    };
    assert!(report(&with(14e6, 13e6, 21)).is_err());
    assert!(report(&with(1e6, 30e6, 21)).is_err());
    assert!(report(&with(13e6, 15e6, 2)).is_err());
    assert!(report(&with(13e6, 15e6, 21)).is_ok());
}

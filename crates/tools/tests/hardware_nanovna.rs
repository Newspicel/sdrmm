#![allow(clippy::expect_used)]

use sdrmm_tools::{NanoVnaTool, Tool};
use sdrmm_wire::{
    NanoVnaCalStep, NanoVnaCalibrateRequest, NanoVnaConfigureRequest, NanoVnaPortRequest,
    NanoVnaRequest, NanoVnaResult, NanoVnaStandard, NanoVnaSweepRequest, NanoVnaSweepState,
    ToolRequest, ToolResponse,
};

fn run(request: NanoVnaRequest) -> NanoVnaResult {
    let response = NanoVnaTool::default()
        .run(ToolRequest::NanoVna(request))
        .expect("the instrument answers");
    let ToolResponse::NanoVna(result) = response else {
        panic!("a NanoVNA request is answered by the NanoVNA tool");
    };
    *result
}

fn port() -> String {
    std::env::var("NANOVNA_PORT").expect("set NANOVNA_PORT to the instrument's serial port")
}

#[test]
#[ignore = "needs a NanoVNA on a serial port"]
fn discovery_finds_the_attached_instrument_and_skips_everything_else() {
    let NanoVnaResult::Devices {
        devices,
        ignored_ports,
    } = run(NanoVnaRequest::ListDevices)
    else {
        panic!("discovery returns devices");
    };
    println!("devices: {devices:#?}\nignored: {ignored_ports:?}");
    assert!(!devices.is_empty(), "no NanoVNA found");
}

#[test]
#[ignore = "needs a NanoVNA on a serial port"]
fn the_instrument_describes_itself() {
    let NanoVnaResult::Device(report) = run(NanoVnaRequest::Describe(NanoVnaPortRequest {
        port: port(),
    })) else {
        panic!("describe returns a report");
    };
    println!("{report:#?}");
    assert!(!report.firmware.is_empty());
    assert!(!report.commands.is_empty());
}

#[test]
#[ignore = "needs a NanoVNA on a serial port"]
fn a_sweep_comes_back_with_the_points_it_was_asked_for() {
    let NanoVnaResult::Sweep(sweep) = run(NanoVnaRequest::Sweep(NanoVnaSweepRequest {
        port: port(),
        start_hz: 1_000_000,
        stop_hz: 30_000_000,
        points: 201,
        averages: 2,
    })) else {
        panic!("a sweep returns points");
    };
    assert_eq!(sweep.points.len(), 201);
    assert_eq!(sweep.points[0].frequency_hz, 1_000_000);
    assert_eq!(sweep.points[200].frequency_hz, 30_000_000);
    println!(
        "{} points in {} ms, firmware {}, cal {}",
        sweep.points.len(),
        sweep.elapsed_ms,
        sweep.device.firmware,
        sweep.device.calibration.raw
    );
    println!("first: {:?}", sweep.points[0]);
}

#[test]
#[ignore = "needs a NanoVNA on a serial port"]
fn a_long_sweep_spans_segments_without_gaps() {
    let NanoVnaResult::Sweep(sweep) = run(NanoVnaRequest::Sweep(NanoVnaSweepRequest {
        port: port(),
        start_hz: 50_000,
        stop_hz: 900_000_000,
        points: 1001,
        averages: 1,
    })) else {
        panic!("a sweep returns points");
    };
    assert_eq!(sweep.points.len(), 1001);
    assert!(
        sweep
            .points
            .windows(2)
            .all(|pair| pair[0].frequency_hz < pair[1].frequency_hz)
    );
    println!("1001 points in {} ms", sweep.elapsed_ms);
}

#[test]
#[ignore = "needs a NanoVNA on a serial port"]
fn bandwidth_changes_and_is_restored() {
    let NanoVnaResult::Device(before) = run(NanoVnaRequest::Describe(NanoVnaPortRequest {
        port: port(),
    })) else {
        panic!("describe returns a report");
    };
    let original = before
        .bandwidth_hz
        .expect("the firmware reports its bandwidth");
    let NanoVnaResult::Device(narrow) = run(NanoVnaRequest::Configure(NanoVnaConfigureRequest {
        port: port(),
        bandwidth_hz: Some(100),
        power: None,
    })) else {
        panic!("configure returns a report");
    };
    assert_eq!(narrow.bandwidth_hz, Some(100));
    let NanoVnaResult::Device(restored) = run(NanoVnaRequest::Configure(NanoVnaConfigureRequest {
        port: port(),
        bandwidth_hz: Some(original),
        power: None,
    })) else {
        panic!("configure returns a report");
    };
    assert_eq!(restored.bandwidth_hz, Some(original));
}

#[test]
#[ignore = "needs a NanoVNA on a serial port"]
fn the_calibration_state_reads_back() {
    let NanoVnaResult::Calibration(state) =
        run(NanoVnaRequest::Calibrate(NanoVnaCalibrateRequest {
            port: port(),
            range: None,
            step: NanoVnaCalStep::Status,
        }))
    else {
        panic!("a calibration step returns the state");
    };
    println!("{state:#?}");
}

#[test]
#[ignore = "needs a NanoVNA on a serial port, and replaces its working calibration with slot 0"]
fn a_one_port_calibration_runs_through_and_slot_zero_comes_back() {
    let calibrate = |step: NanoVnaCalStep, range| {
        let NanoVnaResult::Calibration(state) =
            run(NanoVnaRequest::Calibrate(NanoVnaCalibrateRequest {
                port: port(),
                range,
                step,
            }))
        else {
            panic!("a calibration step returns the state");
        };
        state
    };
    let reset = calibrate(
        NanoVnaCalStep::Reset,
        Some(NanoVnaSweepState {
            start_hz: 1_000_000,
            stop_hz: 30_000_000,
            points: 101,
        }),
    );
    assert!(reset.standards.is_empty() && !reset.applied);
    calibrate(NanoVnaCalStep::Open, None);
    calibrate(NanoVnaCalStep::Short, None);
    let measured = calibrate(NanoVnaCalStep::Load, None);
    assert_eq!(
        measured.standards,
        vec![
            NanoVnaStandard::Load,
            NanoVnaStandard::Open,
            NanoVnaStandard::Short
        ]
    );
    let finished = calibrate(NanoVnaCalStep::Finish, None);
    assert!(finished.applied);
    assert!(finished.error_terms.contains(&"Es".to_owned()));
    assert!(finished.error_terms.contains(&"Er".to_owned()));
    let restored = calibrate(NanoVnaCalStep::Recall { slot: 0 }, None);
    println!("slot 0: {}", restored.raw);
}

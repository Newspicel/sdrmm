use serde_json::json;

use super::*;

fn capabilities(per_stream: bool) -> Capabilities {
    serde_json::from_value(json!({
        "freq_ranges": [{"min": 24e6, "max": 1_766e6}, {"min": 2_000e6, "max": 2_400e6}],
        "sample_rates": [],
        "gains": [],
        "antennas": [],
        "bandwidths": [],
        "per_stream": {"tuning": per_stream},
    }))
    .expect("capabilities")
}

fn bare() -> Capabilities {
    serde_json::from_value(json!({
        "freq_ranges": [], "sample_rates": [], "gains": [], "antennas": [], "bandwidths": [],
    }))
    .expect("capabilities")
}

fn running(center_hz: f64, sample_rate: f64) -> DeviceSet {
    serde_json::from_value(json!({
        "id": 1,
        "device": {"driver": "virtual", "key": "band", "label": "Band"},
        "capabilities": capabilities(false),
        "settings": {"center_hz": center_hz, "sample_rate": sample_rate},
        "status": "running",
        "channels": [],
    }))
    .expect("device set")
}

#[test]
fn a_frequency_out_of_reach_lands_on_the_nearest_edge() {
    let caps = capabilities(false);
    assert!((reachable_hz(&caps, 100e6) - 100e6).abs() < 1.0);
    assert!((reachable_hz(&caps, 1e6) - 24e6).abs() < 1.0);
    assert!((reachable_hz(&caps, 1_900e6) - 2_000e6).abs() < 1.0);
    assert!((reachable_hz(&bare(), 5e6) - 5e6).abs() < 1.0);
}

#[test]
fn tuning_a_lane_leaves_auto_for_that_lane_only() {
    let shared = tune_delta(&capabilities(false), 1, 145e6);
    assert_eq!(shared.center_hz, Some(145e6));
    assert_eq!(shared.tuning, Some(Tuning::Manual));
    assert!(shared.streams.is_empty());
    let split = tune_delta(&capabilities(true), 1, 145e6);
    assert_eq!(split.center_hz, None);
    assert_eq!(split.streams.len(), 1);
    assert_eq!(split.streams[0].stream, 1);
    assert_eq!(split.streams[0].center_hz, Some(145e6));
    assert_eq!(split.streams[0].tuning, Some(Tuning::Manual));
}

#[test]
fn a_lane_reaches_what_fits_inside_its_span_minus_the_channel() {
    let mut live = running(100e6, 2e6);
    assert!(reaches(&live, 0, 200e3, 100.8e6));
    assert!(!reaches(&live, 0, 200e3, 100.95e6));
    live.virtual_lanes.push(sdrmm_wire::VirtualLane {
        stream: 3,
        node: "beam".to_owned(),
        port: "beam".to_owned(),
        center_hz: 433e6,
        sample_rate: 1e6,
    });
    assert!(reaches(&live, 3, 0.0, 433.4e6));
    assert!(!reaches(&live, 3, 0.0, 100e6));
}

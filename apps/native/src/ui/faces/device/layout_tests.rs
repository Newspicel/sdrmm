use serde_json::{Value, json};

use super::*;

fn set(caps: Value, settings: Value) -> DeviceSet {
    let mut base_caps = json!({
        "freq_ranges": [{ "min": 24e6, "max": 1766e6 }], "sample_rates": [], "gains": [],
        "antennas": [], "bandwidths": []
    });
    if let (Some(base), Some(extra)) = (base_caps.as_object_mut(), caps.as_object()) {
        base.extend(extra.clone());
    }
    serde_json::from_value(json!({
        "id": 1,
        "device": { "driver": "kraken", "key": "0", "label": "KrakenSDR" },
        "capabilities": base_caps,
        "settings": settings,
        "status": "running",
        "channels": []
    }))
    .expect("device set")
}

fn bank() -> Value {
    json!({
        "rx_streams": 3,
        "per_stream": { "gain": true, "tuning": true, "agc": true },
        "agc": { "kind": "switch" },
        "gains": [
            { "name": "AMP", "kind": "amp", "range": { "min": 0.0, "max": 14.0 } },
            { "name": "LNA", "kind": "lna", "range": { "min": 0.0, "max": 40.0, "step": 8.0 }, "agc": { "kind": "never" } },
            { "name": "VGA", "kind": "vga", "range": { "min": 0.0, "max": 62.0 } },
            { "name": "TX", "kind": "tx", "range": { "min": 0.0, "max": 47.0 } }
        ]
    })
}

fn names(stages: &[GainStage]) -> Vec<&str> {
    stages.iter().map(|stage| stage.name.as_str()).collect()
}

#[test]
fn a_per_lane_gain_bank_gets_one_row_per_lane_and_a_master() {
    let lanes = set(bank(), json!({}));
    assert_eq!(
        lane_layout(&lanes.capabilities),
        LaneLayout {
            lanes: 3,
            per_lane: true,
            master: true,
            inputs: false
        }
    );
    let single = set(json!({ "rx_inputs": ["A", "B"] }), json!({}));
    assert_eq!(
        lane_layout(&single.capabilities),
        LaneLayout {
            lanes: 1,
            per_lane: false,
            master: false,
            inputs: true
        }
    );
}

#[test]
fn transmit_stages_leave_the_receive_rows_and_the_meter_rides_the_driven_stage() {
    let caps = set(bank(), json!({})).capabilities;
    assert_eq!(names(&rx_stages(&caps)), vec!["AMP", "LNA", "VGA"]);
    assert_eq!(names(&tx_stages(&caps)), vec!["TX"]);
    assert_eq!(agc_stage_index(&rx_stages(&caps)), 0);
    assert_eq!(
        meter_stage(&caps).map(|stage| stage.name),
        Some("LNA".to_owned())
    );
    let driven = set(
        json!({ "gains": [
            { "name": "LNA", "kind": "lna", "range": { "min": 0.0, "max": 40.0 }, "agc": { "kind": "never" } },
            { "name": "IF", "kind": "if", "range": { "min": 0.0, "max": 40.0 } }
        ] }),
        json!({}),
    )
    .capabilities;
    assert_eq!(
        meter_stage(&driven).map(|stage| stage.name),
        Some("IF".to_owned())
    );
}

#[test]
fn every_lane_reads_its_own_gain_and_the_master_shows_their_spread() {
    let lanes = set(
        bank(),
        json!({ "streams": [
            { "stream": 0, "gains": [{ "stage": "VGA", "value_db": 10.0 }] },
            { "stream": 1, "gains": [{ "stage": "VGA", "value_db": 20.0 }] }
        ] }),
    );
    let vga = &rx_stages(&lanes.capabilities)[2];
    assert_eq!(lane_gains(&lanes, vga), vec![10.0, 20.0, 0.0]);
    assert_eq!(
        spread_of(&[10.0, 20.0, 0.0]),
        Spread {
            uniform: false,
            mean: 10.0
        }
    );
    assert!(spread_of(&[5.0, 5.0]).uniform);
    assert_eq!(spread_of(&[]).mean, 0.0);
}

#[test]
fn the_master_sets_every_lane_and_a_lane_sets_only_itself() {
    let caps = set(bank(), json!({})).capabilities;
    let vga = rx_stages(&caps)[2].clone();
    let all = all_lanes_gain(&caps, &vga, 30.0);
    assert_eq!(all.streams.len(), 3);
    assert!(
        all.streams
            .iter()
            .all(|lane| lane.gains[0].value_db == 30.0)
    );
    let one = lane_gain(&caps, 2, &vga, 12.0);
    assert_eq!(one.streams.len(), 1);
    assert_eq!(one.streams[0].stream, 2);
    let plain = set(
        json!({ "gains": [{ "name": "LNA", "kind": "lna", "range": { "min": 0.0, "max": 40.0 } }] }),
        json!({}),
    );
    let lna = rx_stages(&plain.capabilities)[0].clone();
    assert_eq!(
        lane_gain(&plain.capabilities, 0, &lna, 8.0).gains[0].value_db,
        8.0
    );
    assert!(
        all_lanes_gain(&plain.capabilities, &lna, 8.0)
            .streams
            .is_empty()
    );
}

#[test]
fn lanes_take_the_name_of_the_input_feeding_them_and_one_input_stays_on() {
    let inputs = set(
        json!({ "rx_streams": 2, "rx_inputs": ["RX1", "RX2"] }),
        json!({ "rx_inputs": [1, 0] }),
    );
    assert_eq!(lane_input_name(&inputs, 0).as_deref(), Some("RX2"));
    assert_eq!(lane_input_name(&inputs, 1).as_deref(), Some("RX1"));
    assert_eq!(lane_input_name(&inputs, 2), None);
    assert_eq!(
        input_options(&inputs.capabilities),
        vec![(0, "RX1".to_owned()), (1, "RX2".to_owned())]
    );
    assert_eq!(picked_inputs(&DeviceSettings::default()), vec![0]);
    assert_eq!(flip_input(&[0], 1), vec![0, 1]);
    assert_eq!(flip_input(&[0, 1], 0), vec![1]);
    assert_eq!(flip_input(&[1], 1), vec![1]);
}

#[test]
fn a_mode_change_keeps_each_lane_running_or_resting() {
    let lanes = set(
        json!({
            "rx_streams": 2,
            "per_stream": { "agc": true },
            "agc": { "kind": "modes", "options": [{ "value": "fast" }, { "value": "slow" }] }
        }),
        json!({ "streams": [{ "stream": 1, "agc": { "on": true, "mode": "fast" } }] }),
    );
    let delta = agc_mode_delta(&lanes, "slow");
    let modes: Vec<(u32, bool, Option<String>)> = delta
        .streams
        .iter()
        .map(|lane| {
            let agc = lane.agc.clone().unwrap_or_else(AgcSetting::off);
            (lane.stream, agc.on, agc.mode)
        })
        .collect();
    assert_eq!(
        modes,
        vec![
            (0, false, Some("slow".to_owned())),
            (1, true, Some("slow".to_owned()))
        ]
    );
    assert_eq!(radio_agc(&lanes), AgcSetting::in_mode(true, "fast"));
    let whole = set(
        json!({ "agc": { "kind": "modes", "options": [{ "value": "fast" }] } }),
        json!({}),
    );
    assert_eq!(
        agc_mode_delta(&whole, "fast").agc,
        Some(AgcSetting::in_mode(true, "fast"))
    );
    assert_eq!(radio_agc(&whole), AgcSetting::in_mode(false, "fast"));
}

#[test]
fn the_master_auto_is_on_off_or_mixed() {
    assert_eq!(pressed_of(&[true, true]), Pressed::On);
    assert_eq!(pressed_of(&[true, false]), Pressed::Mixed);
    assert_eq!(pressed_of(&[false, false]), Pressed::Off);
    assert_eq!(pressed_of(&[]), Pressed::Off);
}

#[test]
fn a_gain_name_says_who_holds_it_or_that_it_is_not_in_db() {
    assert_eq!(stage_tip(GainUnit::Db, None), None);
    assert_eq!(
        stage_tip(GainUnit::Index, None).as_deref(),
        Some("Firmware step, not dB")
    );
    assert_eq!(
        stage_tip(GainUnit::Db, Some("Kraken array")).as_deref(),
        Some("Set on Kraken array")
    );
}

#[test]
fn linked_lanes_tune_and_lock_together() {
    let lanes = set(bank(), json!({}));
    let caps = &lanes.capabilities;
    let tuned = tune_all_delta(caps, 10e6);
    assert_eq!(tuned.streams.len(), 3);
    assert!(
        tuned
            .streams
            .iter()
            .all(|lane| lane.center_hz == Some(24e6) && lane.tuning == Some(Tuning::Manual))
    );
    assert_eq!(tuning_all_delta(caps, Tuning::Auto).streams.len(), 3);
    assert!(all_auto_tuning(&lanes));
    assert_eq!(lock_all(caps, true), vec![0, 1, 2]);
    assert!(lock_all(caps, false).is_empty());
    assert!(all_locked(&[0, 1, 2], caps));
    assert!(!all_locked(&[0, 2], caps));
    let single = set(json!({}), json!({}));
    assert_eq!(
        tune_all_delta(&single.capabilities, 100e6).center_hz,
        Some(100e6)
    );
}

#[test]
fn a_lossy_link_reads_as_a_whole_percent() {
    let mut lossy = set(json!({}), json!({}));
    assert_eq!(loss_said(&lossy), None);
    lossy.loss = Some(0.126);
    assert_eq!(loss_said(&lossy).as_deref(), Some("13%"));
}

fn graph(nodes: Value, edges: Value) -> PatchGraph {
    serde_json::from_value(json!({ "nodes": nodes, "edges": edges })).expect("graph")
}

#[test]
fn a_lane_wired_to_an_array_is_held_by_it_and_named_after_it() {
    let wired = graph(
        json!([
            { "id": "dev", "kind": "device", "data": {}, "position": { "x": 0.0, "y": 0.0 } },
            { "id": "arr", "kind": "array", "data": {}, "position": { "x": 0.0, "y": 0.0 }, "label": "North" }
        ]),
        json!([
            { "from": { "node": "dev", "port": "iq2" }, "to": { "node": "arr", "port": "lane2" } },
            { "from": { "node": "dev", "port": "iq" }, "to": { "node": "arr", "port": "lane" } }
        ]),
    );
    let holds = lane_holds(&wired, "dev");
    assert_eq!(holds.keys().copied().collect::<Vec<_>>(), vec![0, 1]);
    assert_eq!(holds[&1].label, "North");
    assert_eq!(holds[&1].array, "arr");
    assert_eq!(
        dial_hold(&holds, 1, true).map(|hold| hold.array),
        Some("arr".to_owned())
    );
    assert_eq!(dial_hold(&holds, 2, true), None);
    assert!(dial_hold(&holds, 2, false).is_some());
    assert!(lane_holds(&wired, "arr").is_empty());
}

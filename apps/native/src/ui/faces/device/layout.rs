use std::collections::BTreeMap;

use sdrmm_wire::{
    device::{
        AgcReach, AgcSetting, Capabilities, DeviceSettings, GainKind, GainStage, GainUnit,
        GainValue, StreamSettings, Tuning,
    },
    patch::{NodeBody, PatchGraph, port_stream},
    state::DeviceSet,
};

use super::{
    caps::agc_state,
    lanes::{auto_tuning, lane_agc, rx_stream_count},
};
use crate::ui::kit_sources::dial::reachable_hz;

pub const ARRAY_NAME: &str = "Array";
const ARRAY_LANE_PORT: &str = "lane";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaneLayout {
    pub lanes: u32,
    pub per_lane: bool,
    pub master: bool,
    pub inputs: bool,
}

#[must_use]
pub fn lane_layout(caps: &Capabilities) -> LaneLayout {
    let streams = rx_stream_count(caps);
    let per_lane = caps.per_stream.gain && streams > 1;
    LaneLayout {
        lanes: if per_lane { streams } else { 1 },
        per_lane,
        master: per_lane,
        inputs: caps.rx_inputs.len() > 1,
    }
}

#[must_use]
pub fn rx_stages(caps: &Capabilities) -> Vec<GainStage> {
    caps.gains
        .iter()
        .filter(|stage| stage.kind != GainKind::Tx)
        .cloned()
        .collect()
}

#[must_use]
pub fn tx_stages(caps: &Capabilities) -> Vec<GainStage> {
    caps.gains
        .iter()
        .filter(|stage| stage.kind == GainKind::Tx)
        .cloned()
        .collect()
}

#[must_use]
pub fn agc_stage_index(stages: &[GainStage]) -> usize {
    stages
        .iter()
        .position(|stage| stage.agc != AgcReach::Never)
        .unwrap_or(0)
}

#[must_use]
pub fn meter_stage(caps: &Capabilities) -> Option<GainStage> {
    let stages = rx_stages(caps);
    match stages.get(agc_stage_index(&stages)) {
        Some(driven) if !driven.is_switch() => Some(driven.clone()),
        _ => stages.into_iter().find(|stage| !stage.is_switch()),
    }
}

#[must_use]
pub fn agc_drives(stage: &GainStage, agc: &AgcSetting) -> bool {
    stage.agc.drives(agc)
}

#[must_use]
pub fn lane_gain_db(set: &DeviceSet, stream: u32, stage: &GainStage) -> f64 {
    set.settings
        .for_stream(stream, &set.capabilities.per_stream)
        .gain(&stage.name)
        .unwrap_or(stage.range.min)
}

#[must_use]
pub fn lane_gains(set: &DeviceSet, stage: &GainStage) -> Vec<f64> {
    (0..lane_layout(&set.capabilities).lanes)
        .map(|stream| lane_gain_db(set, stream, stage))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spread {
    pub uniform: bool,
    pub mean: f64,
}

#[must_use]
pub fn spread_of(values: &[f64]) -> Spread {
    let first = values.first().copied().unwrap_or(0.0);
    let mean = if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    };
    Spread {
        uniform: values.iter().all(|value| *value == first),
        mean,
    }
}

fn one_gain(stage: &GainStage, value_db: f64) -> Vec<GainValue> {
    vec![GainValue {
        stage: stage.name.clone(),
        value_db,
    }]
}

#[must_use]
pub fn all_lanes_gain(caps: &Capabilities, stage: &GainStage, value_db: f64) -> DeviceSettings {
    let layout = lane_layout(caps);
    if !layout.per_lane {
        return DeviceSettings {
            gains: one_gain(stage, value_db),
            ..DeviceSettings::default()
        };
    }
    DeviceSettings {
        streams: (0..layout.lanes)
            .map(|stream| StreamSettings {
                stream,
                gains: one_gain(stage, value_db),
                ..StreamSettings::default()
            })
            .collect(),
        ..DeviceSettings::default()
    }
}

#[must_use]
pub fn lane_gain(
    caps: &Capabilities,
    stream: u32,
    stage: &GainStage,
    value_db: f64,
) -> DeviceSettings {
    if !lane_layout(caps).per_lane {
        return DeviceSettings {
            gains: one_gain(stage, value_db),
            ..DeviceSettings::default()
        };
    }
    DeviceSettings {
        streams: vec![StreamSettings {
            stream,
            gains: one_gain(stage, value_db),
            ..StreamSettings::default()
        }],
        ..DeviceSettings::default()
    }
}

#[must_use]
pub fn input_options(caps: &Capabilities) -> Vec<(u32, String)> {
    caps.rx_inputs
        .iter()
        .enumerate()
        .map(|(input, name)| (input as u32, name.clone()))
        .collect()
}

#[must_use]
pub fn picked_inputs(settings: &DeviceSettings) -> Vec<u32> {
    settings.rx_inputs.clone().unwrap_or_else(|| vec![0])
}

#[must_use]
pub fn flip_input(picked: &[u32], input: u32) -> Vec<u32> {
    let mut next: Vec<u32> = picked.iter().copied().filter(|at| *at != input).collect();
    if next.len() == picked.len() {
        next.push(input);
        next.sort_unstable();
    }
    if next.is_empty() {
        picked.to_vec()
    } else {
        next
    }
}

#[must_use]
pub fn lane_input_name(set: &DeviceSet, stream: u32) -> Option<String> {
    let input = *set.settings.rx_inputs.as_ref()?.get(stream as usize)?;
    set.capabilities.rx_inputs.get(input as usize).cloned()
}

#[must_use]
pub fn radio_agc(set: &DeviceSet) -> AgcSetting {
    let caps = &set.capabilities;
    if !caps.per_stream.agc {
        return agc_state(caps, &set.settings);
    }
    let lanes: Vec<AgcSetting> = (0..rx_stream_count(caps))
        .map(|stream| lane_agc(set, stream))
        .collect();
    if let Some(running) = lanes.iter().find(|lane| lane.on) {
        return running.clone();
    }
    AgcSetting {
        on: false,
        ..lanes
            .into_iter()
            .next()
            .unwrap_or_else(|| agc_state(caps, &set.settings))
    }
}

#[must_use]
pub fn agc_mode_delta(set: &DeviceSet, mode: &str) -> DeviceSettings {
    let caps = &set.capabilities;
    if !caps.per_stream.agc {
        return DeviceSettings {
            agc: Some(AgcSetting::in_mode(true, mode)),
            ..DeviceSettings::default()
        };
    }
    DeviceSettings {
        streams: (0..rx_stream_count(caps))
            .map(|stream| StreamSettings {
                stream,
                agc: Some(AgcSetting::in_mode(lane_agc(set, stream).on, mode)),
                ..StreamSettings::default()
            })
            .collect(),
        ..DeviceSettings::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pressed {
    On,
    Mixed,
    Off,
}

#[must_use]
pub fn pressed_of(driven: &[bool]) -> Pressed {
    if !driven.is_empty() && driven.iter().all(|on| *on) {
        Pressed::On
    } else if driven.iter().any(|on| *on) {
        Pressed::Mixed
    } else {
        Pressed::Off
    }
}

#[must_use]
pub fn stage_tip(unit: GainUnit, held_by: Option<&str>) -> Option<String> {
    match (held_by, unit) {
        (Some(array), _) => Some(format!("Set on {array}")),
        (None, GainUnit::Index) => Some("Firmware step, not dB".to_owned()),
        (None, GainUnit::Db) => None,
    }
}

#[must_use]
pub fn tune_all_delta(caps: &Capabilities, hz: f64) -> DeviceSettings {
    let center_hz = Some(reachable_hz(&caps.freq_ranges, hz));
    if !caps.per_stream.tuning {
        return DeviceSettings {
            center_hz,
            tuning: Some(Tuning::Manual),
            ..DeviceSettings::default()
        };
    }
    DeviceSettings {
        streams: (0..rx_stream_count(caps))
            .map(|stream| StreamSettings {
                stream,
                center_hz,
                tuning: Some(Tuning::Manual),
                ..StreamSettings::default()
            })
            .collect(),
        ..DeviceSettings::default()
    }
}

#[must_use]
pub fn tuning_all_delta(caps: &Capabilities, tuning: Tuning) -> DeviceSettings {
    if !caps.per_stream.tuning {
        return DeviceSettings {
            tuning: Some(tuning),
            ..DeviceSettings::default()
        };
    }
    DeviceSettings {
        streams: (0..rx_stream_count(caps))
            .map(|stream| StreamSettings {
                stream,
                tuning: Some(tuning),
                ..StreamSettings::default()
            })
            .collect(),
        ..DeviceSettings::default()
    }
}

#[must_use]
pub fn all_auto_tuning(set: &DeviceSet) -> bool {
    (0..rx_stream_count(&set.capabilities)).all(|stream| auto_tuning(set, stream))
}

#[must_use]
pub fn lock_all(caps: &Capabilities, held: bool) -> Vec<u32> {
    if held {
        (0..rx_stream_count(caps)).collect()
    } else {
        Vec::new()
    }
}

#[must_use]
pub fn all_locked(locked: &[u32], caps: &Capabilities) -> bool {
    (0..rx_stream_count(caps)).all(|stream| locked.contains(&stream))
}

#[must_use]
pub fn loss_said(set: &DeviceSet) -> Option<String> {
    set.loss
        .map(|loss| format!("{}%", (f64::from(loss) * 100.0).round()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneHold {
    pub array: String,
    pub label: String,
}

fn array_name(graph: &PatchGraph, array: &str) -> String {
    graph
        .node(array)
        .and_then(|node| node.label.clone())
        .unwrap_or_else(|| ARRAY_NAME.to_owned())
}

#[must_use]
pub fn lane_holds(graph: &PatchGraph, device: &str) -> BTreeMap<u32, LaneHold> {
    let mut holds = BTreeMap::new();
    for edge in &graph.edges {
        let Some(stream) = (edge.from.node == device)
            .then(|| port_stream("iq", &edge.from.port))
            .flatten()
        else {
            continue;
        };
        let into_array = port_stream(ARRAY_LANE_PORT, &edge.to.port).is_some()
            && graph
                .node(&edge.to.node)
                .is_some_and(|node| matches!(node.body, NodeBody::Array(_)));
        if into_array && !holds.contains_key(&stream) {
            holds.insert(
                stream,
                LaneHold {
                    array: edge.to.node.clone(),
                    label: array_name(graph, &edge.to.node),
                },
            );
        }
    }
    holds
}

#[must_use]
pub fn dial_hold(holds: &BTreeMap<u32, LaneHold>, stream: u32, merged: bool) -> Option<LaneHold> {
    if merged {
        holds.get(&stream).cloned()
    } else {
        holds.values().next().cloned()
    }
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;

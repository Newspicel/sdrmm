use sdrmm_wire::{
    ARRAY_PORT, ArrayTuneRequest, Capabilities, ChannelSettings, DeviceSet, DeviceSettings,
    IQ_PORT, NodeBody, PatchGraph, ResolvedPatch, Role, StreamSettings, Tuning,
};

use super::{node_of, takes_no_such_action};
use crate::{AppState, array, resolved::Resolved, rest::AppError};

const LOCKED: &str = "Tuning locked";

#[derive(Debug, PartialEq)]
pub(super) enum Lane {
    Radio { node: String, set: u32, stream: u32 },
    Array { node: String, set: u32, stream: u32 },
}

pub(super) fn tune(
    state: &AppState,
    node: &str,
    freq_hz: f64,
    author: Option<&str>,
) -> Result<(), AppError> {
    if !freq_hz.is_finite() || freq_hz <= 0.0 {
        return Err(AppError::bad_request("A frequency has to be positive"));
    }
    let resolved = state.resolved.current();
    let body = &node_of(&resolved, node)?.body;
    match body {
        NodeBody::Channel(channel) if channel.tuning_locked => Err(AppError::bad_request(LOCKED)),
        NodeBody::Channel(channel) => {
            pull_lane(
                state,
                &resolved,
                node,
                &channel.channel_type,
                freq_hz,
                author,
            )?;
            retune_channel(state, node, freq_hz, author)
        }
        NodeBody::Array(_) => tune_array(state, node, freq_hz),
        body if body.opens_device() => {
            let set = resolved
                .patch
                .device_set(node)
                .ok_or_else(|| AppError::bad_request(format!("{node} has no radio open")))?;
            tune_radio(state, &resolved.graph, node, set, 0, freq_hz, author)
        }
        _ => match lane_of(&resolved.patch, node) {
            Some(lane) => tune_lane(state, &resolved.graph, &lane, freq_hz, author),
            None => Err(takes_no_such_action(node)),
        },
    }
}

pub(super) fn lane_of(patch: &ResolvedPatch, node: &str) -> Option<Lane> {
    let feed = patch.first_feed(node, IQ_PORT)?;
    let (set, stream) = feed.tap.as_ref()?.lane()?;
    let source = patch.node(&feed.source)?;
    if source.kind.role() == Role::OpensRadio {
        return Some(Lane::Radio {
            node: feed.source.clone(),
            set,
            stream,
        });
    }
    let array = patch.first_feed(&feed.source, ARRAY_PORT)?;
    Some(Lane::Array {
        node: array.source.clone(),
        set,
        stream,
    })
}

fn tune_lane(
    state: &AppState,
    graph: &PatchGraph,
    lane: &Lane,
    freq_hz: f64,
    author: Option<&str>,
) -> Result<(), AppError> {
    match lane {
        Lane::Radio { node, set, stream } => {
            tune_radio(state, graph, node, *set, *stream, freq_hz, author)
        }
        Lane::Array { node, .. } => tune_array(state, node, freq_hz),
    }
}

fn tune_array(state: &AppState, node: &str, freq_hz: f64) -> Result<(), AppError> {
    array::tune(
        state,
        node,
        ArrayTuneRequest {
            center_hz: Some(freq_hz),
            gain: None,
        },
    )
}

fn tune_radio(
    state: &AppState,
    graph: &PatchGraph,
    node: &str,
    set: u32,
    stream: u32,
    freq_hz: f64,
    author: Option<&str>,
) -> Result<(), AppError> {
    if lane_locked(graph, node, stream) {
        return Err(AppError::bad_request(LOCKED));
    }
    let live = state.engine.snapshot();
    let capabilities = &live
        .device_sets
        .iter()
        .find(|held| held.id == set)
        .ok_or_else(|| AppError::bad_request(format!("{node} has no radio open")))?
        .capabilities;
    let delta = tune_delta(capabilities, stream, freq_hz);
    crate::rest::patch_device_live(state, set, delta, author)
}

fn lane_locked(graph: &PatchGraph, node: &str, stream: u32) -> bool {
    graph.node(node).is_some_and(|held| match &held.body {
        NodeBody::Device(device) => device.tuning_locked(stream),
        _ => false,
    })
}

pub(super) fn tune_delta(capabilities: &Capabilities, stream: u32, freq_hz: f64) -> DeviceSettings {
    let center_hz = Some(reachable_hz(capabilities, freq_hz));
    if capabilities.per_stream.tuning {
        return DeviceSettings {
            streams: vec![StreamSettings {
                stream,
                center_hz,
                tuning: Some(Tuning::Manual),
                ..StreamSettings::default()
            }],
            ..DeviceSettings::default()
        };
    }
    DeviceSettings {
        center_hz,
        tuning: Some(Tuning::Manual),
        ..DeviceSettings::default()
    }
}

pub(super) fn reachable_hz(capabilities: &Capabilities, freq_hz: f64) -> f64 {
    capabilities
        .freq_ranges
        .iter()
        .map(|range| freq_hz.clamp(range.min, range.max.max(range.min)))
        .min_by(|a, b| (a - freq_hz).abs().total_cmp(&(b - freq_hz).abs()))
        .unwrap_or(freq_hz)
}

fn pull_lane(
    state: &AppState,
    resolved: &Resolved,
    node: &str,
    channel_type: &str,
    freq_hz: f64,
    author: Option<&str>,
) -> Result<(), AppError> {
    if !resolved.patch.retunes_source(node, IQ_PORT) {
        return Ok(());
    }
    let Some(lane) = lane_of(&resolved.patch, node) else {
        return Ok(());
    };
    let (set, stream) = match &lane {
        Lane::Radio { set, stream, .. } | Lane::Array { set, stream, .. } => (*set, *stream),
    };
    let live = state.engine.snapshot();
    let Some(held) = live.device_sets.iter().find(|held| held.id == set) else {
        return Ok(());
    };
    let bandwidth_hz = state
        .engine
        .channel_types()
        .iter()
        .find(|descriptor| descriptor.type_id == channel_type)
        .map_or(0.0, |descriptor| descriptor.bandwidth_hz);
    let follows = matches!(lane, Lane::Radio { .. }) && auto_tuning(held, stream);
    if follows || reaches(held, stream, bandwidth_hz, freq_hz) {
        return Ok(());
    }
    tune_lane(state, &resolved.graph, &lane, freq_hz, author)
}

fn auto_tuning(set: &DeviceSet, stream: u32) -> bool {
    set.settings
        .for_stream(stream, &set.capabilities.per_stream)
        .tuning
        .unwrap_or_default()
        == Tuning::Auto
}

pub(super) fn reaches(set: &DeviceSet, stream: u32, bandwidth_hz: f64, freq_hz: f64) -> bool {
    let (center_hz, span_hz) = match set.virtual_lanes.iter().find(|lane| lane.stream == stream) {
        Some(lane) => (Some(lane.center_hz), Some(lane.sample_rate)),
        None => (
            set.settings
                .for_stream(stream, &set.capabilities.per_stream)
                .center_hz,
            set.settings.sample_rate,
        ),
    };
    let (Some(center_hz), Some(span_hz)) = (center_hz, span_hz) else {
        return true;
    };
    if !center_hz.is_finite() || !span_hz.is_finite() || span_hz <= 0.0 {
        return true;
    }
    let limit_hz = ((span_hz - bandwidth_hz) / 2.0).max(0.0);
    (freq_hz - center_hz).abs() <= limit_hz
}

fn retune_channel(
    state: &AppState,
    node: &str,
    freq_hz: f64,
    author: Option<&str>,
) -> Result<(), AppError> {
    let edit = |current: ChannelSettings| ChannelSettings {
        frequency_hz: freq_hz,
        ..current
    };
    crate::rest::edit_channel(state, node, edit, author)
}

#[cfg(test)]
mod tests;

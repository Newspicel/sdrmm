use sdrmm_wire::{
    BASEBAND_PORT, DeviceSet, IQ_PORT, NetworkExportAction, NetworkExportSettings,
    NetworkExportStatus, NodeBody, PlaybackRequest, PlaybackStatus, StateScope, StateSnapshot,
    TimeMachineAction, TimeMachineNode, TimeMachineStatus,
};

use super::{node_of, takes_no_such_action};
use crate::{
    AppState,
    resolved::Resolved,
    rest::{AppError, lock_gate, reconcile_recordings},
};

const WIRE_IQ: &str = "Wire a running radio's IQ into it";

pub(super) fn time_machine(
    state: &AppState,
    node: &str,
    action: TimeMachineAction,
) -> Result<TimeMachineStatus, AppError> {
    let resolved = state.resolved.current();
    let NodeBody::TimeMachine(settings) = &node_of(&resolved, node)?.body else {
        return Err(takes_no_such_action(node));
    };
    let (set, stream) = holding_history(&state.engine.snapshot(), node)
        .or_else(|| fed_lane(&resolved, node))
        .ok_or_else(|| AppError::bad_request(WIRE_IQ))?;
    let status = drive_history(state, node, set, stream, action, *settings)?;
    state.gps.route_current(state);
    Ok(status)
}

fn drive_history(
    state: &AppState,
    node: &str,
    set: u32,
    stream: u32,
    action: TimeMachineAction,
    settings: TimeMachineNode,
) -> Result<TimeMachineStatus, AppError> {
    let finalizes = matches!(action, TimeMachineAction::Stop | TimeMachineAction::Disarm);
    let status =
        state
            .engine
            .control_time_machine(set, node.to_owned(), stream, action, settings)?;
    if finalizes && let Some(dir) = state.engine.recordings_dir() {
        {
            let _gate = lock_gate(&state.recordings_gate);
            reconcile_recordings(dir, &state.store)?;
        }
        state.engine.emit_scope(StateScope::Recordings);
    }
    Ok(status)
}

fn holding_history(live: &StateSnapshot, node: &str) -> Option<(u32, u32)> {
    live.device_sets.iter().find_map(|set| {
        set.time_machine
            .as_ref()
            .filter(|held| held.node == node)
            .map(|held| (set.id, held.stream))
    })
}

fn fed_lane(resolved: &Resolved, node: &str) -> Option<(u32, u32)> {
    resolved
        .patch
        .first_feed(node, IQ_PORT)?
        .tap
        .as_ref()?
        .lane()
}

pub(super) fn network_export(
    state: &AppState,
    node: &str,
    action: NetworkExportAction,
) -> Result<NetworkExportStatus, AppError> {
    let resolved = state.resolved.current();
    let NodeBody::NetworkExport(body) = &node_of(&resolved, node)?.body else {
        return Err(takes_no_such_action(node));
    };
    match action {
        NetworkExportAction::Start => start_export(state, &resolved, node, body.settings.clone()),
        NetworkExportAction::Stop => stop_export(state, node),
    }
}

fn start_export(
    state: &AppState,
    resolved: &Resolved,
    node: &str,
    settings: NetworkExportSettings,
) -> Result<NetworkExportStatus, AppError> {
    let engine = &state.engine;
    if let Some((set, stream)) = fed_lane(resolved, node) {
        return Ok(engine.start_network_export(set, node.to_owned(), stream, settings)?);
    }
    let channel = resolved
        .patch
        .feeds(node, BASEBAND_PORT)
        .iter()
        .find_map(|feed| feed.tap.as_ref()?.channels().into_iter().next())
        .ok_or_else(|| {
            AppError::bad_request("Wire a radio's IQ or a channel's baseband into it")
        })?;
    Ok(engine.start_channel_network_export(
        channel.device_set,
        channel.channel,
        node.to_owned(),
        settings,
    )?)
}

fn stop_export(state: &AppState, node: &str) -> Result<NetworkExportStatus, AppError> {
    let live = state.engine.snapshot();
    let exports = |set: &DeviceSet| {
        set.network_export
            .as_ref()
            .is_some_and(|export| export.node == node)
    };
    if let Some(set) = live.device_sets.iter().find(|set| exports(set)) {
        return Ok(state.engine.stop_network_export(set.id, node)?);
    }
    let channel = live.device_sets.iter().find_map(|set| {
        set.channels
            .iter()
            .find(|channel| {
                channel
                    .network_export
                    .as_ref()
                    .is_some_and(|export| export.node == node)
            })
            .map(|channel| (set.id, channel.id))
    });
    let (set, channel) = channel.ok_or_else(|| AppError::bad_request("Not exporting"))?;
    Ok(state
        .engine
        .stop_channel_network_export(set, channel, node)?)
}

pub(super) fn playback(
    state: &AppState,
    node: &str,
    request: &PlaybackRequest,
) -> Result<PlaybackStatus, AppError> {
    let resolved = state.resolved.current();
    if !matches!(node_of(&resolved, node)?.body, NodeBody::Recording(_)) {
        return Err(takes_no_such_action(node));
    }
    let set = resolved
        .patch
        .device_set(node)
        .ok_or_else(|| AppError::bad_request(format!("{node} has no recording open")))?;
    Ok(state.engine.control_playback(set, request)?)
}

use sdrmm_wire::{
    ChannelSettings, DeviceSettings, NodeAction, ResolvedPatch, ScanAction, ScannerStatus,
    StateSnapshot, SweepStatus,
};

use super::{
    args::{NodeView, WorkspaceView},
    canvas,
};
use crate::{
    AppState,
    rest::{self, AppError},
    workspace,
};

pub(super) fn workspace_view(state: &AppState) -> Result<WorkspaceView, AppError> {
    let detail = canvas::active(state)?;
    let patch = workspace::resolve(&detail.snapshot.graph, &state.engine.snapshot());
    let running = patch
        .radios()
        .into_iter()
        .map(|(node, _)| node)
        .chain(patch.channels().into_iter().map(|(node, _)| node))
        .collect();
    Ok(WorkspaceView { detail, running })
}

pub(super) fn node_view(state: &AppState, id: &str) -> Result<NodeView, AppError> {
    let detail = canvas::active(state)?;
    let graph = &detail.snapshot.graph;
    let node = graph.node(id).ok_or_else(|| canvas::unknown(id))?.clone();
    let snapshot = state.engine.snapshot();
    let patch = workspace::resolve(graph, &snapshot);
    let radio = patch
        .device_set(id)
        .and_then(|device_set| set(&snapshot, device_set).cloned());
    let decoder = decoder_of(&patch, id).and_then(|(ds, ch)| {
        set(&snapshot, ds)?
            .channels
            .iter()
            .find(|channel| channel.id == ch)
            .cloned()
    });
    let scan = snapshot
        .device_sets
        .iter()
        .flat_map(|set| &set.scanners)
        .find(|scanner| scanner.settings.node.as_deref() == Some(id))
        .cloned();
    let saved = detail
        .state
        .channel(id)
        .map(|channel| channel.settings.clone());
    Ok(NodeView {
        node,
        radio,
        decoder,
        saved,
        scan,
    })
}

pub(super) fn radio_of(state: &AppState, id: &str) -> Result<u32, AppError> {
    let graph = canvas::active(state)?.snapshot.graph;
    let node = graph.node(id).ok_or_else(|| canvas::unknown(id))?;
    if !node.body.opens_device() {
        return Err(AppError::bad_request(format!(
            "{id} is a {} node, not a radio",
            node.body.kind()
        )));
    }
    workspace::resolve(&graph, &state.engine.snapshot())
        .device_set(id)
        .ok_or_else(|| AppError::bad_request(format!("{id} has no radio open")))
}

pub(super) fn tune(
    state: &AppState,
    id: &str,
    settings: DeviceSettings,
) -> Result<DeviceSettings, AppError> {
    let device_set = radio_of(state, id)?;
    rest::patch_device_live(state, device_set, settings, Some(super::MCP_AUTHOR))?;
    set(&state.engine.snapshot(), device_set)
        .map(|set| set.settings.clone())
        .ok_or_else(|| AppError::bad_request(format!("{id} closed its radio")))
}

pub(super) fn set_channel(
    state: &AppState,
    id: &str,
    settings: ChannelSettings,
) -> Result<bool, AppError> {
    let detail = canvas::active(state)?;
    let graph = &detail.snapshot.graph;
    rest::check_channel_node(graph, id, &settings)?;
    match decoder_of(&workspace::resolve(graph, &state.engine.snapshot()), id) {
        Some((ds, ch)) => {
            rest::patch_channel_live(state, ds, ch, settings, Some(super::MCP_AUTHOR))?;
            Ok(true)
        }
        None => {
            rest::save_channel(state, detail.info.id, id, settings)?;
            Ok(false)
        }
    }
}

pub(super) fn scan(
    state: &AppState,
    id: &str,
    action: ScanAction,
) -> Result<ScannerStatus, AppError> {
    rest::succeeded(rest::act(state, id, NodeAction::Scan(action))?)?
        .into_iter()
        .find_map(|ack| match ack.sweep.map(|sweep| *sweep) {
            Some(SweepStatus::Scan(status)) => Some(status),
            _ => None,
        })
        .ok_or_else(|| AppError::bad_request(format!("{id} drives no scan")))
}

fn decoder_of(patch: &ResolvedPatch, id: &str) -> Option<(u32, u32)> {
    patch
        .channel(id)
        .map(|addr| (addr.device_set, addr.channel))
}

fn set(snapshot: &StateSnapshot, device_set: u32) -> Option<&sdrmm_wire::DeviceSet> {
    snapshot.device_sets.iter().find(|set| set.id == device_set)
}

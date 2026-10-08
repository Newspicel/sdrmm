use sdrmm_wire::{
    ChannelSettings, DeviceSettings, NodeBody, PatchGraph, ScanAction, ScanRequest, ScannerStatus,
    StateSnapshot,
};

use super::{
    args::{NodeView, WorkspaceView},
    canvas,
};
use crate::{
    AppState,
    rest::{self, AppError},
    workspace::{self, DeviceBinding},
};

const CONTROL_PORT: &str = "control";

pub(super) fn workspace_view(state: &AppState) -> Result<WorkspaceView, AppError> {
    let detail = canvas::active(state)?;
    let running = workspace::bind(&detail.snapshot.graph, &state.engine.snapshot())
        .into_iter()
        .flat_map(|binding| {
            std::iter::once(binding.node).chain(binding.channels.into_iter().map(|(node, _)| node))
        })
        .collect();
    Ok(WorkspaceView { detail, running })
}

pub(super) fn node_view(state: &AppState, id: &str) -> Result<NodeView, AppError> {
    let detail = canvas::active(state)?;
    let graph = &detail.snapshot.graph;
    let node = graph.node(id).ok_or_else(|| canvas::unknown(id))?.clone();
    let snapshot = state.engine.snapshot();
    let bindings = workspace::bind(graph, &snapshot);
    let radio = bindings
        .iter()
        .find(|binding| binding.node == id)
        .and_then(|binding| set(&snapshot, binding.device_set).cloned());
    let decoder = decoder_of(&bindings, id).and_then(|(ds, ch)| {
        set(&snapshot, ds)?
            .channels
            .iter()
            .find(|channel| channel.id == ch)
            .cloned()
    });
    let scan = scan_target(graph, &node.body, id)
        .and_then(|target| decoder_of(&bindings, target))
        .and_then(|(ds, ch)| {
            set(&snapshot, ds)?
                .scanners
                .iter()
                .find(|scanner| scanner.settings.channel == ch)
                .cloned()
        });
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
    workspace::bind_devices(&graph, &state.engine.snapshot())
        .into_iter()
        .find(|(node, _)| node == id)
        .map(|(_, device_set)| device_set)
        .ok_or_else(|| AppError::bad_request(format!("{id} has no radio open")))
}

pub(super) fn tune(
    state: &AppState,
    id: &str,
    settings: DeviceSettings,
) -> Result<DeviceSettings, AppError> {
    let device_set = radio_of(state, id)?;
    rest::patch_device_live(state, device_set, settings)?;
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
    match decoder_of(&workspace::bind(graph, &state.engine.snapshot()), id) {
        Some((ds, ch)) => {
            rest::patch_channel_live(state, ds, ch, settings)?;
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
    let graph = canvas::active(state)?.snapshot.graph;
    let node = graph.node(id).ok_or_else(|| canvas::unknown(id))?;
    let NodeBody::Scanner(scanner) = &node.body else {
        return Err(AppError::bad_request(format!(
            "{id} is a {} node, not a scanner",
            node.body.kind()
        )));
    };
    let target = scan_target(&graph, &node.body, id).ok_or_else(|| {
        AppError::bad_request(format!("wire the control output of {id} to a decoder"))
    })?;
    let (ds, ch) = decoder_of(&workspace::bind(&graph, &state.engine.snapshot()), target)
        .ok_or_else(|| AppError::bad_request(format!("{target} runs on no radio")))?;
    let request = ScanRequest {
        action,
        settings: Some(scanner.settings.clone()),
    };
    rest::scan(&state.engine, ds, ch, request)
}

fn scan_target<'a>(graph: &'a PatchGraph, body: &NodeBody, id: &'a str) -> Option<&'a str> {
    matches!(body, NodeBody::Scanner(_))
        .then(|| graph.targets_of(id, CONTROL_PORT).next())
        .flatten()
}

fn decoder_of(bindings: &[DeviceBinding], id: &str) -> Option<(u32, u32)> {
    bindings.iter().find_map(|binding| {
        binding
            .channels
            .iter()
            .find(|(node, _)| node == id)
            .map(|(_, channel)| (binding.device_set, *channel))
    })
}

fn set(snapshot: &StateSnapshot, device_set: u32) -> Option<&sdrmm_wire::DeviceSet> {
    snapshot.device_sets.iter().find(|set| set.id == device_set)
}

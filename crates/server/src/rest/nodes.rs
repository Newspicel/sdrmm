use sdrmm_wire::{ChannelInfo, NetworkRadioRequest};

use super::*;

#[utoipa::path(
    patch, path = "/api/workspaces/active/nodes/{node}/channel",
    params(("node" = String, Path, description = "Channel node of the active workspace")),
    request_body = PatchChannelRequest,
    responses(
        (
            status = 204,
            description = "Settings applied to the running decoder, or held for the node until a \
                           radio carries it. With a base, only the fields changed from it land",
        ),
        (status = 400, description = "Not a channel node, settings of another channel type, or a \
                                      new frequency for a held decoder", body = ApiError),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn patch_node_channel(
    State(state): State<AppState>,
    Path(node): Path<String>,
    author: Author,
    Json(req): Json<PatchChannelRequest>,
) -> Result<StatusCode, AppError> {
    tokio::task::spawn_blocking(move || {
        set_channel(
            &state,
            &node,
            req.settings,
            req.base.as_ref(),
            author.as_deref(),
        )
    })
    .await??;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) fn set_channel(
    state: &AppState,
    node: &str,
    settings: ChannelSettings,
    base: Option<&ChannelSettings>,
    author: Option<&str>,
) -> Result<bool, AppError> {
    let _serialized = lock_gate(&state.apply_gate);
    set_channel_locked(
        state,
        node,
        settings,
        base,
        Tuner {
            author,
            actor: None,
        },
    )
}

fn set_channel_locked(
    state: &AppState,
    node: &str,
    settings: ChannelSettings,
    base: Option<&ChannelSettings>,
    tuner: Tuner<'_>,
) -> Result<bool, AppError> {
    let active = active_workspace(&state.store)?;
    check_channel_node(&active.snapshot.graph, node, &settings)?;
    let live = state.engine.snapshot();
    let Some((set, channel)) = live_channel(&live, node) else {
        if let Some(saved) = active.state.channel(node) {
            may_retune(state, node, &saved.settings, &settings, tuner.actor)?;
        }
        hold_channel(state, active.info.id, node, settings)?;
        return Ok(false);
    };
    let settings = match base {
        Some(base) => crate::merge::merge_typed(base, &settings, &channel.settings)
            .map_err(|err| AppError::bad_request(format!("settings do not merge: {err}")))?,
        None => settings,
    };
    may_retune(state, node, &channel.settings, &settings, tuner.actor)?;
    patch_channel_locked(state, set, channel.id, settings, tuner.author)?;
    Ok(true)
}

fn may_retune(
    state: &AppState,
    node: &str,
    current: &ChannelSettings,
    next: &ChannelSettings,
    actor: Option<&str>,
) -> Result<(), AppError> {
    if current.frequency_hz.to_bits() == next.frequency_hz.to_bits() {
        return Ok(());
    }
    state
        .resolved
        .current()
        .patch
        .may_tune(node, actor)
        .map_err(AppError::bad_request)
}

pub(crate) fn edit_channel(
    state: &AppState,
    node: &str,
    edit: impl FnOnce(ChannelSettings) -> ChannelSettings,
    tuner: Tuner<'_>,
) -> Result<(), AppError> {
    let _serialized = lock_gate(&state.apply_gate);
    let current = channel_settings(state, node)?;
    set_channel_locked(state, node, edit(current), None, tuner).map(drop)
}

pub(crate) fn channel_settings(state: &AppState, node: &str) -> Result<ChannelSettings, AppError> {
    if let Some((_, channel)) = live_channel(&state.engine.snapshot(), node) {
        return Ok(channel.settings.clone());
    }
    let active = active_workspace(&state.store)?;
    if let Some(saved) = active.state.channel(node) {
        return Ok(saved.settings.clone());
    }
    let channel_type = match active.snapshot.graph.node(node).map(|held| &held.body) {
        Some(NodeBody::Channel(channel)) => channel.channel_type.clone(),
        _ => return Err(AppError::bad_request(format!("{node:?} is not a channel"))),
    };
    state
        .engine
        .channel_types()
        .into_iter()
        .find(|descriptor| descriptor.type_id == channel_type)
        .and_then(|descriptor| descriptor.defaults)
        .ok_or_else(|| AppError::bad_request(format!("No decoder {channel_type}")))
}

fn live_channel<'a>(live: &'a StateSnapshot, node: &str) -> Option<(u32, &'a ChannelInfo)> {
    live.device_sets.iter().find_map(|set| {
        set.channels
            .iter()
            .find(|channel| channel.node.as_deref() == Some(node))
            .map(|channel| (set.id, channel))
    })
}

fn hold_channel(
    state: &AppState,
    id: i64,
    node: &str,
    settings: ChannelSettings,
) -> Result<(), AppError> {
    let mut saved = state.store.workspace_state(id)?;
    saved.put_channel(node, settings);
    state.store.put_workspace_state(id, &saved)?;
    state.engine.emit_scope(StateScope::Workspace(id));
    Ok(())
}

#[utoipa::path(
    patch, path = "/api/workspaces/active/nodes/{node}/device",
    params(("node" = String, Path, description = "Radio node of the active workspace")),
    request_body = DeviceSettings,
    responses(
        (status = 204, description = "Settings applied to the node's radio"),
        (status = 400, description = "Not a radio node, no radio open, or an unsupported setting",
         body = ApiError),
        (status = 404, description = "No such node", body = ApiError),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn patch_node_device(
    State(state): State<AppState>,
    Path(node): Path<String>,
    author: Author,
    Json(settings): Json<DeviceSettings>,
) -> Result<StatusCode, AppError> {
    tokio::task::spawn_blocking(move || {
        set_device(&state, &node, settings, author.as_deref()).map(drop)
    })
    .await??;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) fn set_device(
    state: &AppState,
    node: &str,
    settings: DeviceSettings,
    author: Option<&str>,
) -> Result<u32, AppError> {
    let set = radio_of(state, node)?;
    patch_device_live(state, set, settings, author)?;
    Ok(set)
}

pub(crate) fn radio_of(state: &AppState, node: &str) -> Result<u32, AppError> {
    let graph = active_workspace(&state.store)?.snapshot.graph;
    let held = graph
        .node(node)
        .ok_or_else(|| AppError::not_found(format!("No node {node:?}")))?;
    if !held.body.opens_device() {
        return Err(AppError::bad_request(format!("{node} is not a radio")));
    }
    workspace::resolve(&graph, &state.engine.snapshot())
        .device_set(node)
        .ok_or_else(|| AppError::bad_request(format!("{node} has no radio open")))
}

#[utoipa::path(
    post, path = "/api/devices/network",
    request_body = NetworkRadioRequest,
    responses(
        (status = 200, description = "The network radio, known now but not opened. Name it in a \
                                      Device node to open it", body = DeviceInfo),
        (status = 404, description = "No radio answers at that address", body = ApiError),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn add_network_radio(
    State(state): State<AppState>,
    Json(req): Json<NetworkRadioRequest>,
) -> Result<Json<DeviceInfo>, AppError> {
    let engine = state.engine.clone();
    let address = req.address.trim().to_owned();
    let device = tokio::task::spawn_blocking(move || engine.adopt_device(&address))
        .await?
        .ok_or_else(|| AppError::not_found(format!("No radio answers at {}", req.address)))?;
    Ok(Json(device))
}

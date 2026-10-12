use super::*;

#[utoipa::path(
    get, path = "/api/presets",
    responses((status = 200, description = "Stored presets", body = Vec<PresetInfo>)),
)]
pub(super) async fn list_presets(
    State(state): State<AppState>,
) -> Result<Json<Vec<PresetInfo>>, AppError> {
    let store = state.store.clone();
    let presets = tokio::task::spawn_blocking(move || store.list_presets()).await??;
    Ok(Json(presets))
}

#[utoipa::path(
    post, path = "/api/presets",
    request_body = CreatePresetRequest,
    responses(
        (status = 200, description = "Preset stored", body = CreatedRowId),
        (
            status = 400,
            description = "No active workspace, or none of its device nodes is on a live radio",
            body = ApiError,
        ),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn create_preset(
    State(state): State<AppState>,
    Json(req): Json<CreatePresetRequest>,
) -> Result<Json<CreatedRowId>, AppError> {
    let engine = state.engine.clone();
    let store = state.store.clone();
    let id = tokio::task::spawn_blocking(move || -> Result<i64, AppError> {
        let active = store
            .active_workspace()?
            .ok_or_else(|| AppError::bad_request("no active workspace to snapshot".to_owned()))?;
        let live = engine.snapshot();
        let devices: Vec<PresetDevice> = workspace::bind(&active.snapshot.graph, &live)
            .into_iter()
            .filter_map(|binding| {
                let set = live
                    .device_sets
                    .iter()
                    .find(|set| set.id == binding.device_set)?;
                Some(PresetDevice {
                    device_id: set.device.id(),
                    settings: set.settings.clone(),
                    channels: binding
                        .channels
                        .iter()
                        .filter_map(|(_, id)| set.channels.iter().find(|c| c.id == *id))
                        .map(|c| c.settings.clone())
                        .collect(),
                    node: binding.node,
                })
            })
            .collect();
        if devices.is_empty() {
            return Err(AppError::bad_request(
                "no radio on this workspace is open, so there is nothing to save".to_owned(),
            ));
        }
        let snapshot = PresetSnapshot {
            version: PRESET_SNAPSHOT_VERSION,
            devices,
        };
        let id = store.create_preset(&req.name, &snapshot)?;
        engine.emit_scope(StateScope::Presets);
        Ok(id)
    })
    .await??;
    Ok(Json(CreatedRowId { id }))
}

#[utoipa::path(
    post, path = "/api/presets/{id}/apply",
    params(("id" = i64, Path, description = "Preset id")),
    responses(
        (status = 204, description = "Preset applied"),
        (
            status = 400,
            description = "Preset rejected by a target radio, or none of its radios is on this \
                           workspace; `detail` reports what state a partial application left \
                           behind",
            body = ApiError,
        ),
        (status = 404, description = "Preset not found", body = ApiError),
    ),
)]
pub(super) async fn apply_preset(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let app = state.clone();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let _serialized = lock_gate(&app.apply_gate);
        let snapshot = app.store.preset_snapshot(id)?;
        if snapshot.version != PRESET_SNAPSHOT_VERSION {
            return Err(AppError::bad_request(format!(
                "preset {id} has unsupported snapshot version {} (this build applies \
                 {PRESET_SNAPSHOT_VERSION})",
                snapshot.version
            )));
        }
        let active = app.store.active_workspace()?.ok_or_else(|| {
            AppError::bad_request("no active workspace to apply a preset to".to_owned())
        })?;
        let targets = preset_targets(&active.snapshot.graph, &app.engine.snapshot(), snapshot);
        if targets.is_empty() {
            return Err(AppError::bad_request(format!(
                "preset {id} names no radio this workspace has open"
            )));
        }
        let total = targets.len();
        for (done, (device_set, device)) in targets.iter().enumerate() {
            let settings = device.settings.clone().calibrated(&calibration::of(
                &app.engine,
                &app.store,
                *device_set,
            ));
            configure_radio(&app.engine, *device_set, settings, &device.channels).map_err(
                |err| {
                    err.with_detail(format!(
                        "{done} of {total} radios in the preset were configured"
                    ))
                },
            )?;
        }
        place_merged(&app, &preset_merge(id, &targets))
    })
    .await??;
    resolve_now(state).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn preset_targets(
    graph: &PatchGraph,
    live: &StateSnapshot,
    snapshot: PresetSnapshot,
) -> Vec<(u32, PresetDevice)> {
    let bindings = workspace::bind_devices(graph, live);
    let mut targets: Vec<(u32, PresetDevice)> = Vec::new();
    for device in snapshot.devices {
        let free = |set: u32| !targets.iter().any(|(taken, _)| *taken == set);
        let found = bindings
            .iter()
            .find(|(node, set)| *node == device.node && free(*set))
            .or_else(|| {
                bindings.iter().find(|(_, set)| {
                    free(*set)
                        && live
                            .device_sets
                            .iter()
                            .any(|open| open.id == *set && open.device.id() == device.device_id)
                })
            });
        if let Some((_, set)) = found {
            targets.push((*set, device));
        }
    }
    targets
}

fn configure_radio(
    engine: &sdrmm_engine::Engine,
    ds: u32,
    settings: DeviceSettings,
    channels: &[ChannelSettings],
) -> Result<(), AppError> {
    engine.validate_configuration(ds, &settings, channels)?;
    engine.patch_device(ds, settings)?;
    Ok(())
}

const PRESET_COLUMN: f32 = 520.0;
const PRESET_ROW: f32 = 240.0;

fn preset_merge(id: i64, targets: &[(u32, PresetDevice)]) -> Merge {
    let mut patch = PatchGraph::default();
    let mut radios = Vec::new();
    let mut settings = Vec::new();
    for (radio, (device_set, device)) in targets.iter().enumerate() {
        let dev = format!("{radio}:dev");
        patch.nodes.push(preset_node(
            &dev,
            NodeBody::Device(sdrmm_wire::DeviceNode::default()),
            0.0,
            settings.len(),
        ));
        for (index, channel) in device.channels.iter().enumerate() {
            let node = format!("{radio}:ch{index}");
            patch.nodes.push(preset_node(
                &node,
                NodeBody::Channel(sdrmm_wire::ChannelNode {
                    channel_type: channel.params.type_id().to_owned(),
                    tuning_locked: false,
                }),
                PRESET_COLUMN,
                settings.len(),
            ));
            patch.edges.push(sdrmm_wire::PatchEdge {
                from: sdrmm_wire::PortRef {
                    node: dev.clone(),
                    port: "iq".to_owned(),
                },
                to: sdrmm_wire::PortRef {
                    node: node.clone(),
                    port: "iq".to_owned(),
                },
            });
            settings.push((node, channel.clone()));
        }
        radios.push((dev, *device_set));
    }
    Merge {
        patch,
        prefix: format!("preset:{id}:"),
        radios,
        settings,
    }
}

fn preset_node(id: &str, body: NodeBody, x: f32, row: usize) -> sdrmm_wire::PatchNode {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a preset holds a handful of decoders; the row index is exact in f32"
    )]
    let y = row as f32 * PRESET_ROW;
    sdrmm_wire::PatchNode {
        id: id.to_owned(),
        body,
        position: sdrmm_wire::Position { x, y },
        size: None,
        label: None,
    }
}

#[utoipa::path(
    delete, path = "/api/presets/{id}",
    params(("id" = i64, Path, description = "Preset id")),
    responses(
        (status = 204, description = "Preset removed"),
        (status = 400, description = "Invalid path parameter", body = ApiError),
        (status = 404, description = "Preset not found", body = ApiError),
    ),
)]
pub(super) async fn delete_preset(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let engine = state.engine.clone();
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        store.delete_preset(id)?;
        engine.emit_scope(StateScope::Presets);
        Ok(())
    })
    .await??;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/api/bookmarks",
    responses((status = 200, description = "Stored bookmarks", body = Vec<Bookmark>)),
)]
pub(super) async fn list_bookmarks(
    State(state): State<AppState>,
) -> Result<Json<Vec<Bookmark>>, AppError> {
    let store = state.store.clone();
    let bookmarks = tokio::task::spawn_blocking(move || store.list_bookmarks()).await??;
    Ok(Json(bookmarks))
}

#[utoipa::path(
    post, path = "/api/bookmarks",
    request_body = CreateBookmarkRequest,
    responses(
        (status = 200, description = "Bookmark stored", body = CreatedRowId),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn create_bookmark(
    State(state): State<AppState>,
    Json(req): Json<CreateBookmarkRequest>,
) -> Result<Json<CreatedRowId>, AppError> {
    let engine = state.engine.clone();
    let store = state.store.clone();
    let id = tokio::task::spawn_blocking(move || -> Result<i64, AppError> {
        let id = store.create_bookmark(&req)?;
        engine.emit_scope(StateScope::Bookmarks);
        Ok(id)
    })
    .await??;
    Ok(Json(CreatedRowId { id }))
}

#[utoipa::path(
    delete, path = "/api/bookmarks/{id}",
    params(("id" = i64, Path, description = "Bookmark id")),
    responses(
        (status = 204, description = "Bookmark removed"),
        (status = 400, description = "Invalid path parameter", body = ApiError),
        (status = 404, description = "Bookmark not found", body = ApiError),
    ),
)]
pub(super) async fn delete_bookmark(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let engine = state.engine.clone();
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        store.delete_bookmark(id)?;
        engine.emit_scope(StateScope::Bookmarks);
        Ok(())
    })
    .await??;
    Ok(StatusCode::NO_CONTENT)
}

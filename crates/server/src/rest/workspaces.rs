use super::*;
use crate::apply::{bring_up, bring_up_active};
use sdrmm_wire::ResolvedState;

#[utoipa::path(
    get, path = "/api/templates",
    responses((
        status = 200,
        description = "Built-in workspace templates (read-only; presets are the writable kind)",
        body = TemplatesResponse,
    )),
)]
pub(super) async fn list_templates(
    State(state): State<AppState>,
) -> Result<Json<TemplatesResponse>, AppError> {
    let engine = state.engine.clone();
    let probed = tokio::task::spawn_blocking(move || engine.probe_devices()).await?;
    let open = state.engine.snapshot().device_sets;
    let templates = crate::templates::all()
        .iter()
        .map(|template| TemplateInfo {
            supported_devices: devices_running(template, &probed, &open),
            ..template.clone()
        })
        .collect();
    Ok(Json(TemplatesResponse { templates }))
}

fn devices_running(
    template: &TemplateInfo,
    probed: &[DeviceInfo],
    open: &[sdrmm_wire::DeviceSet],
) -> Vec<String> {
    let probed = probed
        .iter()
        .filter(|device| {
            device
                .profile
                .as_ref()
                .is_none_or(|profile| template.unmet_by(profile).is_none())
        })
        .map(DeviceInfo::id);
    let open = open
        .iter()
        .filter(|set| template.unmet_by(&set.capabilities.profile()).is_none())
        .map(|set| set.device.id());
    let mut ids: Vec<String> = probed.chain(open).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

#[utoipa::path(
    post, path = "/api/templates/{id}/apply",
    params(("id" = String, Path, description = "Template id")),
    request_body = ApplyTemplateRequest,
    responses(
        (status = 204, description = "Template applied"),
        (
            status = 400,
            description = "Template rejected by the target device (usually out of its tuning \
                           range); `detail` reports what a partial application left behind",
            body = ApiError,
        ),
        (status = 404, description = "No such template or node", body = ApiError),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn apply_template(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ApplyTemplateRequest>,
) -> Result<StatusCode, AppError> {
    let template = crate::templates::get(&id)
        .ok_or_else(|| AppError::not_found(format!("template {id} not found")))?;
    let app = state.clone();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let _serialized = lock_gate(&app.apply_gate);
        let device_set = radio_of(&app, &req.node)?;
        let merge = template_merge(template, device_set)?;
        let settings = template_settings(&app.engine, template, device_set)?;
        app.engine
            .validate_configuration(device_set, &settings, &template.channels)?;
        app.engine.patch_device(device_set, settings)?;
        place_merged(&app, &merge)
    })
    .await??;
    resolve_now(state).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn template_settings(
    engine: &sdrmm_engine::Engine,
    template: &TemplateInfo,
    device_set: u32,
) -> Result<DeviceSettings, AppError> {
    let open = engine.snapshot();
    let mut rate = template.sample_rate;
    if let Some(set) = open.device_sets.iter().find(|set| set.id == device_set) {
        let profile = set.capabilities.profile();
        if let Some(reason) = template.unmet_by(&profile) {
            return Err(AppError::bad_request(format!(
                "{} cannot run this template: {reason}",
                set.device.label
            )));
        }
        rate = template.rate_on(&profile).unwrap_or(rate);
    }
    Ok(DeviceSettings {
        center_hz: Some(template.center_hz),
        sample_rate: Some(rate),
        tuning: (!template.channels.is_empty()).then_some(sdrmm_wire::Tuning::Auto),
        ..DeviceSettings::default()
    })
}

fn template_merge(template: &TemplateInfo, device_set: u32) -> Result<Merge, AppError> {
    let patch = template
        .patch
        .as_ref()
        .ok_or_else(|| AppError::internal(format!("template {} has no patch", template.id)))?;
    let radios = patch
        .device_nodes()
        .map(|node| (node.id.clone(), device_set))
        .take(1)
        .collect();
    Ok(Merge {
        settings: template_decoders(template, patch)?,
        patch: patch.clone(),
        prefix: format!("template:{}:", template.id),
        radios,
    })
}

fn template_decoders(
    template: &TemplateInfo,
    patch: &PatchGraph,
) -> Result<Vec<(String, ChannelSettings)>, AppError> {
    let nodes: Vec<(&str, &str)> = patch
        .nodes
        .iter()
        .filter_map(|node| match &node.body {
            NodeBody::Channel(channel) => Some((node.id.as_str(), channel.channel_type.as_str())),
            _ => None,
        })
        .collect();
    let paired = nodes.len() == template.channels.len()
        && nodes
            .iter()
            .zip(&template.channels)
            .all(|((_, channel_type), settings)| settings.params.type_id() == *channel_type);
    if !paired {
        return Err(AppError::internal(format!(
            "template {} draws other decoders than it configures",
            template.id
        )));
    }
    Ok(nodes
        .into_iter()
        .zip(&template.channels)
        .map(|((node, _), settings)| (node.to_owned(), settings.clone()))
        .collect())
}

pub(crate) fn active_workspace(store: &Store) -> Result<WorkspaceDetail, AppError> {
    store
        .active_workspace()?
        .ok_or_else(|| AppError::bad_request("no active workspace".to_owned()))
}

pub(super) struct Merge {
    pub patch: PatchGraph,
    pub prefix: String,
    pub radios: Vec<(String, u32)>,
    pub settings: Vec<(String, ChannelSettings)>,
}

pub(super) fn place_merged(app: &AppState, merge: &Merge) -> Result<(), AppError> {
    let mut active = active_workspace(&app.store)?;
    merge_onto(&mut active.snapshot, merge, &app.engine.snapshot());
    let id = active.info.id;
    let update = UpdateWorkspaceRequest {
        revision: active.info.revision,
        name: None,
        snapshot: Some(active.snapshot),
    };
    app.store.update_workspace(id, &update, None)?;
    let placed: Vec<(String, ChannelSettings)> = merge
        .settings
        .iter()
        .map(|(node, settings)| (format!("{}{node}", merge.prefix), settings.clone()))
        .collect();
    hold_settings(app, id, &placed)?;
    app.engine.emit_scope(StateScope::Workspaces);
    let report = bring_up_active(app, id)?;
    refused_among(&report, &placed)
}

fn merge_onto(snapshot: &mut WorkspaceSnapshot, merge: &Merge, live: &StateSnapshot) {
    let bound = workspace::bind_devices(&snapshot.graph, live);
    let mut patch = merge.patch.clone();
    let mut reused: Vec<(String, String)> = Vec::new();
    for (device, set) in &merge.radios {
        if let Some((node, _)) = bound
            .iter()
            .find(|(node, held)| held == set && !node.starts_with(&merge.prefix))
        {
            reused.push((device.clone(), node.clone()));
            continue;
        }
        let named = live
            .device_sets
            .iter()
            .find(|open| open.id == *set)
            .map(|open| sdrmm_wire::DeviceRef::from_info(&open.device));
        if let Some(node) = patch.nodes.iter_mut().find(|node| node.id == *device)
            && let NodeBody::Device(body) = &mut node.body
        {
            body.device = named;
        }
    }
    patch
        .nodes
        .retain(|node| !reused.iter().any(|(device, _)| *device == node.id));
    snapshot.merge_patch(&patch, &merge.prefix, None);
    let placed = |id: &str| {
        reused
            .iter()
            .find(|(device, _)| device == id)
            .map_or_else(|| format!("{}{id}", merge.prefix), |(_, node)| node.clone())
    };
    let to_reused = merge.patch.edges.iter().filter(|edge| {
        reused
            .iter()
            .any(|(device, _)| *device == edge.from.node || *device == edge.to.node)
    });
    for edge in to_reused {
        let wired = sdrmm_wire::PatchEdge {
            from: sdrmm_wire::PortRef {
                node: placed(&edge.from.node),
                port: edge.from.port.clone(),
            },
            to: sdrmm_wire::PortRef {
                node: placed(&edge.to.node),
                port: edge.to.port.clone(),
            },
        };
        if !snapshot.graph.edges.contains(&wired) {
            snapshot.graph.edges.push(wired);
        }
    }
}

fn hold_settings(
    app: &AppState,
    id: i64,
    placed: &[(String, ChannelSettings)],
) -> Result<(), AppError> {
    let live = app.engine.snapshot();
    let resolved = app.resolved.current();
    let placed: Vec<(String, ChannelSettings)> = placed
        .iter()
        .map(|(node, settings)| {
            (
                node.clone(),
                kept_on(&resolved.patch, &live, node, settings),
            )
        })
        .collect();
    let mut saved = app.store.workspace_state(id)?;
    for (node, settings) in &placed {
        saved.put_channel(node, settings.clone());
    }
    app.store.put_workspace_state(id, &saved)?;
    for (node, settings) in &placed {
        let carried = live.device_sets.iter().flat_map(|set| {
            set.channels
                .iter()
                .filter(|channel| {
                    channel.node.as_deref() == Some(node.as_str())
                        && channel.settings.params.type_id() == settings.params.type_id()
                })
                .map(|channel| (set.id, channel.id))
        });
        for (set, channel) in carried {
            app.engine.patch_channel(set, channel, settings.clone())?;
        }
    }
    Ok(())
}

fn kept_on(
    patch: &sdrmm_wire::ResolvedPatch,
    live: &StateSnapshot,
    node: &str,
    settings: &ChannelSettings,
) -> ChannelSettings {
    let heard = live
        .device_sets
        .iter()
        .flat_map(|set| &set.channels)
        .find(|channel| channel.node.as_deref() == Some(node));
    match heard {
        Some(heard) if patch.may_tune(node, None).is_err() => ChannelSettings {
            frequency_hz: heard.settings.frequency_hz,
            ..settings.clone()
        },
        _ => settings.clone(),
    }
}

fn refused_among(
    report: &PatchApplyReport,
    placed: &[(String, ChannelSettings)],
) -> Result<(), AppError> {
    let refused: Vec<String> = report
        .refused
        .iter()
        .filter(|refusal| placed.iter().any(|(node, _)| *node == refusal.node))
        .map(|refusal| format!("{}: {}", refusal.node, refusal.reason))
        .collect();
    if refused.is_empty() {
        return Ok(());
    }
    Err(AppError::bad_request(refused.join("; "))
        .with_detail("the decoders are on the canvas but did not all open".to_owned()))
}

#[utoipa::path(
    get, path = "/api/workspaces",
    responses((
        status = 200,
        description = "Stored workspaces and which one is active. Layouts are not included: \
                       fetch one workspace for that",
        body = WorkspacesResponse,
    )),
)]
pub(super) async fn list_workspaces(
    State(state): State<AppState>,
) -> Result<Json<WorkspacesResponse>, AppError> {
    let store = state.store.clone();
    let workspaces = tokio::task::spawn_blocking(move || store.list_workspaces()).await??;
    Ok(Json(workspaces))
}

#[utoipa::path(
    post, path = "/api/workspaces",
    request_body = CreateWorkspaceRequest,
    responses(
        (status = 200, description = "Workspace stored", body = CreatedRowId),
        (status = 400, description = "Layout rejected", body = ApiError),
        (status = 409, description = "A workspace of that name already exists", body = ApiError),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn create_workspace(
    State(state): State<AppState>,
    Json(req): Json<CreateWorkspaceRequest>,
) -> Result<Json<CreatedRowId>, AppError> {
    let engine = state.engine.clone();
    let store = state.store.clone();
    let id = tokio::task::spawn_blocking(move || -> Result<i64, AppError> {
        let snapshot = req.snapshot.unwrap_or_else(WorkspaceSnapshot::empty);
        let id = store.create_workspace(&req.name, &snapshot)?;
        engine.emit_scope(StateScope::Workspaces);
        Ok(id)
    })
    .await??;
    Ok(Json(CreatedRowId { id }))
}

#[utoipa::path(
    get, path = "/api/workspaces/{id}",
    params(("id" = i64, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "The workspace and its layout", body = WorkspaceDetail),
        (status = 404, description = "Workspace not found", body = ApiError),
        (
            status = 500,
            description = "The stored layout no longer parses: the row is left intact so a \
                           newer build can still read it",
            body = ApiError,
        ),
    ),
)]
pub(super) async fn get_workspace(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    author: Author,
) -> Result<Json<WorkspaceDetail>, AppError> {
    let store = state.store.clone();
    let workspace =
        tokio::task::spawn_blocking(move || store.workspace_for(id, author.as_deref())).await??;
    Ok(Json(workspace))
}

#[utoipa::path(
    get, path = "/api/workspaces/{id}/export",
    params(("id" = i64, Path, description = "Workspace id")),
    responses(
        (
            status = 200,
            description = "The workspace as a portable document: its name, the patch and rack it \
                           draws, and the tuning each node was left on. Nothing server-local \
                           travels, no id, revision or history, so importing it makes a new \
                           workspace rather than overwriting one",
            body = WorkspaceExport,
        ),
        (status = 400, description = "Invalid path parameter", body = ApiError),
        (status = 404, description = "Workspace not found", body = ApiError),
        (
            status = 500,
            description = "The stored layout no longer parses: the row is left intact so a \
                           newer build can still read it",
            body = ApiError,
        ),
    ),
)]
pub(super) async fn export_workspace(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let store = state.store.clone();
    let export = tokio::task::spawn_blocking(move || store.export_workspace(id)).await??;
    let filename = export_filename(&export.name, id);
    let body = serde_json::to_string_pretty(&export)
        .map_err(|err| AppError::internal(format!("serializing the workspace export: {err}")))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/json".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        body,
    )
        .into_response())
}

pub(super) fn export_filename(name: &str, id: i64) -> String {
    let mut slug = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        format!("workspace-{id}.json")
    } else {
        format!("workspace-{slug}.json")
    }
}

#[utoipa::path(
    post, path = "/api/workspaces/import",
    request_body = WorkspaceExport,
    responses(
        (
            status = 200,
            description = "Imported as a new workspace, keeping the one it was exported from. A \
                           name already in use gains a copy number; the radios it names are \
                           opened by activating and applying it, and the ones this machine does \
                           not have are reported absent",
            body = CreatedRowId,
        ),
        (
            status = 400,
            description = "Not a workspace document this build can read, or its layout is \
                           rejected",
            body = ApiError,
        ),
        (status = 409, description = "No free name is left for this one", body = ApiError),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn import_workspace(
    State(state): State<AppState>,
    Json(mut document): Json<serde_json::Value>,
) -> Result<Json<CreatedRowId>, AppError> {
    let notices = crate::store::upgrade_import(&mut document);
    let export: WorkspaceExport = crate::json::from_value(&document).map_err(|err| {
        rejection(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid request body",
            err.to_string(),
        )
    })?;
    let engine = state.engine.clone();
    let store = state.store.clone();
    let id = tokio::task::spawn_blocking(move || -> Result<i64, AppError> {
        let id = store.import_workspace(&export, &notices)?;
        engine.emit_scope(StateScope::Workspaces);
        Ok(id)
    })
    .await??;
    Ok(Json(CreatedRowId { id }))
}

#[utoipa::path(
    delete, path = "/api/workspaces/{id}/notices/{notice}",
    params(
        ("id" = i64, Path, description = "Workspace id"),
        ("notice" = i64, Path, description = "Notice id"),
    ),
    responses(
        (status = 204, description = "Notice dismissed"),
        (status = 400, description = "Invalid path parameter", body = ApiError),
        (status = 404, description = "Workspace or notice not found", body = ApiError),
    ),
)]
pub(super) async fn dismiss_workspace_notice(
    State(state): State<AppState>,
    Path((id, notice)): Path<(i64, i64)>,
) -> Result<StatusCode, AppError> {
    let engine = state.engine.clone();
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        store.dismiss_notice(id, notice)?;
        engine.emit_scope(StateScope::Workspaces);
        Ok(())
    })
    .await??;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    put, path = "/api/workspaces/{id}",
    params(("id" = i64, Path, description = "Workspace id")),
    request_body = UpdateWorkspaceRequest,
    responses(
        (
            status = 200,
            description = "The workspace as stored. A snapshot sent against an older revision is \
                           merged with what other clients wrote since",
            body = WorkspaceDetail,
        ),
        (status = 400, description = "Layout rejected", body = ApiError),
        (status = 404, description = "Workspace not found", body = ApiError),
        (
            status = 409,
            description = "The revision is too old to merge, the merge broke the layout, or the \
                           name is taken",
            body = ApiError,
        ),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn update_workspace(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    author: Author,
    Json(req): Json<UpdateWorkspaceRequest>,
) -> Result<Json<WorkspaceDetail>, AppError> {
    let engine = state.engine.clone();
    let store = state.store.clone();
    let app = state.clone();
    let detail = tokio::task::spawn_blocking(move || -> Result<WorkspaceDetail, AppError> {
        let _serialized = lock_gate(&app.apply_gate);
        let active = store.active_workspace_id()? == Some(id);
        let before = active.then(|| store.workspace(id)).transpose()?;
        if let (Some(before), Some(wanted)) = (&before, &req.snapshot)
            && crate::apply::drops_radio(&app, &before.snapshot.graph, &wanted.graph)
        {
            workspace::save_active(&app)?;
        }
        let detail = store.update_workspace(id, &req, author.as_deref())?;
        if let Some(before) = before {
            crate::apply::release(&app, &before.snapshot.graph, &detail.snapshot.graph);
        }
        engine.emit_scope(StateScope::Workspaces);
        Ok(detail)
    })
    .await??;
    resolve_now(state).await?;
    Ok(Json(detail))
}

#[utoipa::path(
    delete, path = "/api/workspaces/{id}",
    params(("id" = i64, Path, description = "Workspace id")),
    responses(
        (status = 204, description = "Workspace removed"),
        (status = 400, description = "Invalid path parameter", body = ApiError),
        (status = 404, description = "Workspace not found", body = ApiError),
    ),
)]
pub(super) async fn delete_workspace(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    author: Author,
) -> Result<StatusCode, AppError> {
    let gps_state = state.clone();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let _serialized = lock_gate(&state.apply_gate);
        let before = state.store.active_workspace_id()?;
        let after = state.store.delete_workspace(id)?;
        if let Some(promoted) = after.filter(|promoted| Some(*promoted) != before) {
            let detail = state.store.workspace(promoted)?;
            let saved = state.store.workspace_state(promoted)?;
            workspace::reconcile(&state, &detail.snapshot.graph, &saved);
            let report = bring_up_active(&state, promoted)?;
            state.engine.emit_event(ServerEvent::WorkspaceSwitched {
                id: promoted,
                by: author.name(&state),
                report,
            });
        }
        state.engine.emit_scope(StateScope::Workspaces);
        Ok(())
    })
    .await??;
    resolve_now(gps_state).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/api/workspaces/{id}/activate",
    params(("id" = i64, Path, description = "Workspace id")),
    responses(
        (
            status = 200,
            description = "Workspace activated for every client and brought up once: radios it \
                           does not name are closed, channels it does not draw are dropped, the \
                           rest are opened where it was left. Every client hears \
                           WorkspaceSwitched with the same report",
            body = PatchApplyReport,
        ),
        (status = 400, description = "Invalid path parameter", body = ApiError),
        (status = 404, description = "Workspace not found", body = ApiError),
    ),
)]
pub(super) async fn activate_workspace(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    author: Author,
) -> Result<Json<PatchApplyReport>, AppError> {
    let gps_state = state.clone();
    let report = tokio::task::spawn_blocking(move || {
        let _serialized = lock_gate(&state.apply_gate);
        let by = author.name(&state);
        switch(&state, id, by)
    })
    .await??;
    resolve_now(gps_state).await?;
    Ok(Json(report))
}

pub(crate) fn switch(
    state: &AppState,
    id: i64,
    by: Option<String>,
) -> Result<PatchApplyReport, AppError> {
    activate(state, id)?;
    let report = bring_up_active(state, id)?;
    state.engine.emit_event(ServerEvent::WorkspaceSwitched {
        id,
        by,
        report: report.clone(),
    });
    Ok(report)
}

pub(crate) fn activate(state: &AppState, id: i64) -> Result<(), AppError> {
    if let Err(err) = workspace::save_active(state) {
        tracing::warn!(%err, "could not save the outgoing workspace before the switch");
    }
    state.store.activate_workspace(id)?;
    let detail = state.store.workspace(id)?;
    let saved = state.store.workspace_state(id)?;
    let report = workspace::reconcile(state, &detail.snapshot.graph, &saved);
    tracing::info!(
        workspace = id,
        closed = report.closed,
        channels = report.dropped_channels,
        scans = report.stopped_scans,
        "activated"
    );
    state.engine.emit_scope(StateScope::Workspaces);
    Ok(())
}

#[utoipa::path(
    post, path = "/api/workspaces/{id}/undo",
    params(("id" = i64, Path, description = "Workspace id")),
    responses(
        (
            status = 200,
            description = "Takes back the caller's own last change, keeping what others did \
                           since. Callers are told apart by the x-sdrmm-author header",
            body = WorkspaceDetail,
        ),
        (status = 400, description = "Invalid path parameter", body = ApiError),
        (status = 404, description = "Workspace not found", body = ApiError),
        (status = 409, description = "Nothing left to undo", body = ApiError),
    ),
)]
pub(super) async fn undo_workspace(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    author: Author,
) -> Result<Json<WorkspaceDetail>, AppError> {
    step_history(state, id, author.into_key(), Store::undo_workspace).await
}

#[utoipa::path(
    post, path = "/api/workspaces/{id}/redo",
    params(("id" = i64, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "Puts back the caller's last undone change", body = WorkspaceDetail),
        (status = 400, description = "Invalid path parameter", body = ApiError),
        (status = 404, description = "Workspace not found", body = ApiError),
        (status = 409, description = "Nothing left to redo", body = ApiError),
    ),
)]
pub(super) async fn redo_workspace(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    author: Author,
) -> Result<Json<WorkspaceDetail>, AppError> {
    step_history(state, id, author.into_key(), Store::redo_workspace).await
}

pub(crate) type HistoryStep = fn(&Store, i64, Option<&str>) -> Result<SteppedWorkspace, StoreError>;

pub(crate) async fn step_history(
    state: AppState,
    id: i64,
    author: Option<String>,
    step: HistoryStep,
) -> Result<Json<WorkspaceDetail>, AppError> {
    let gps_state = state.clone();
    let detail = tokio::task::spawn_blocking(move || -> Result<WorkspaceDetail, AppError> {
        let _serialized = lock_gate(&state.apply_gate);
        let before = state.store.workspace(id)?;
        if state.store.active_workspace_id()? == Some(id) {
            workspace::save_active(&state)?;
        }
        let stepped = step(&state.store, id, author.as_deref())?;
        let detail = stepped.detail;
        if state.store.active_workspace_id()? == Some(id) {
            crate::apply::release(&state, &before.snapshot.graph, &detail.snapshot.graph);
            if !before.snapshot.graph.same_topology(&detail.snapshot.graph) {
                let saved = state.store.workspace_state(id)?;
                workspace::reconcile(&state, &detail.snapshot.graph, &saved);
                let report = bring_up(&state, id, &detail.snapshot, &saved)?;
                for refusal in &report.refused {
                    tracing::warn!(
                        workspace = id,
                        node = refusal.node,
                        reason = refusal.reason,
                        "a node could not be restored by the history step"
                    );
                }
            }
            if let Some(settings) = &stepped.settings {
                workspace::restore_settings(&state, &detail.snapshot.graph, settings);
            }
        }
        state.engine.emit_scope(StateScope::Workspaces);
        Ok(detail)
    })
    .await??;
    resolve_now(gps_state).await?;
    Ok(Json(detail))
}

#[utoipa::path(
    post, path = "/api/workspaces/{id}/apply",
    params(("id" = i64, Path, description = "Workspace id")),
    responses(
        (
            status = 200,
            description = "The workspace was brought up: radios opened, channels added, and what \
                           could not be satisfied. Additive and idempotent: nothing is closed \
                           or deleted, so calling it twice changes nothing",
            body = PatchApplyReport,
        ),
        (status = 400, description = "Invalid path parameter", body = ApiError),
        (status = 404, description = "Workspace not found", body = ApiError),
    ),
)]
pub(super) async fn apply_workspace(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<PatchApplyReport>, AppError> {
    let gps_state = state.clone();
    let report = tokio::task::spawn_blocking(move || {
        let _serialized = lock_gate(&state.apply_gate);
        bring_up_active(&state, id)
    })
    .await??;
    resolve_now(gps_state).await?;
    Ok(Json(report))
}

pub(crate) async fn resolve_now(state: AppState) -> Result<(), AppError> {
    tokio::task::spawn_blocking(move || state.resolve_now()).await?;
    Ok(())
}

#[utoipa::path(
    get, path = "/api/workspaces/active/resolved",
    responses((
        status = 200,
        description = "The active workspace as the server resolved it: concrete ports, what feeds \
                       each input, what each output reaches, where each node runs, and node \
                       problems",
        body = ResolvedState,
    )),
)]
pub(super) async fn get_resolved(State(state): State<AppState>) -> Json<ResolvedState> {
    Json(ResolvedState {
        patch: state.resolved.current().patch.clone(),
        problems: state.problems.all(),
    })
}

/// Refuses settings that do not belong to the node they are addressed to. A decoder holds its own
/// settings, so whether a radio is wired into it yet is none of this check's business.
pub(crate) fn check_channel_node(
    graph: &PatchGraph,
    node: &str,
    settings: &ChannelSettings,
) -> Result<(), AppError> {
    let Some(patch) = graph.node(node) else {
        return Err(AppError::bad_request(format!("no node {node:?}")));
    };
    let NodeBody::Channel(channel) = &patch.body else {
        return Err(AppError::bad_request(format!("{node:?} is not a channel")));
    };
    if channel.channel_type != settings.params.type_id() {
        return Err(AppError::bad_request(format!(
            "{node:?} is a {} channel, not {}",
            channel.channel_type,
            settings.params.type_id()
        )));
    }
    Ok(())
}

use super::*;

#[utoipa::path(
    get, path = "/api/state",
    responses((status = 200, description = "Full state snapshot", body = StateSnapshot)),
)]
pub(super) async fn get_state(State(state): State<AppState>) -> Json<StateSnapshot> {
    Json(state.engine.snapshot())
}

#[utoipa::path(
    get, path = "/api/devices",
    responses((status = 200, description = "Discovered devices", body = DevicesResponse)),
)]
pub(super) async fn get_devices(
    State(state): State<AppState>,
) -> Result<Json<DevicesResponse>, AppError> {
    let engine = state.engine.clone();
    let devices = tokio::task::spawn_blocking(move || engine.probe_devices()).await?;
    let claims = sdrmm_wire::device_claims(&state.resolved.current().graph, &devices);
    Ok(Json(DevicesResponse { devices, claims }))
}

#[utoipa::path(
    get, path = "/api/position/nmea-devices",
    responses(
        (status = 200, description = "Serial devices available to NMEA GPS nodes", body = NmeaDevicesResponse),
        (status = 500, description = "Serial device discovery failed", body = ApiError),
    ),
)]
pub(super) async fn get_nmea_devices() -> Result<Json<NmeaDevicesResponse>, AppError> {
    let devices = tokio::task::spawn_blocking(crate::gps::nmea_devices)
        .await?
        .map_err(AppError::internal)?;
    Ok(Json(devices))
}

#[utoipa::path(
    post, path = "/api/devices/serial",
    request_body = WriteSerialRequest,
    responses(
        (status = 200, description = "Serial written, takes effect after a replug", body = WrittenSerial),
        (status = 400, description = "This radio cannot take that serial", body = ApiError),
        (status = 404, description = "Device not found", body = ApiError),
        (status = 409, description = "Device open here or in another program", body = ApiError),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn write_serial(
    State(state): State<AppState>,
    Json(req): Json<WriteSerialRequest>,
) -> Result<Json<WrittenSerial>, AppError> {
    let engine = state.engine.clone();
    let serial = tokio::task::spawn_blocking(move || {
        engine.write_serial(&req.device_id, req.serial.as_deref())
    })
    .await??;
    Ok(Json(WrittenSerial { serial }))
}

pub(crate) fn patch_device_live(
    state: &AppState,
    ds: u32,
    settings: DeviceSettings,
    author: Option<&str>,
) -> Result<(), AppError> {
    let _serialized = lock_gate(&state.apply_gate);
    let edit = workspace::begin_edit(state, ds, None, author);
    let calibrates = settings.ppm.is_some() || settings.offset_hz.is_some();
    state.engine.patch_device(ds, settings)?;
    if calibrates {
        calibration::remember_live(&state.engine, &state.store, ds);
    }
    if let Some(edit) = edit {
        workspace::finish_edit(state, edit);
    }
    crate::placement::settle_active(state);
    Ok(())
}

pub(super) fn patch_channel_locked(
    state: &AppState,
    ds: u32,
    ch: u32,
    settings: ChannelSettings,
    author: Option<&str>,
) -> Result<(), AppError> {
    let edit = workspace::begin_edit(state, ds, Some(ch), author);
    state.engine.patch_channel(ds, ch, settings)?;
    if let Some(edit) = edit {
        workspace::finish_edit(state, edit);
    }
    crate::placement::settle_active(state);
    Ok(())
}

#[utoipa::path(
    get, path = "/api/channeltypes",
    responses((status = 200, description = "Available channel types", body = ChannelTypesResponse)),
)]
pub(super) async fn get_channel_types(State(state): State<AppState>) -> Json<ChannelTypesResponse> {
    Json(ChannelTypesResponse {
        types: state.engine.channel_types(),
    })
}

#[utoipa::path(
    get, path = "/api/saved-radios",
    responses((status = 200, description = "Saved network radios", body = Vec<SavedRadio>)),
)]
pub(super) async fn list_saved_radios(
    State(state): State<AppState>,
) -> Result<Json<Vec<SavedRadio>>, AppError> {
    let store = state.store.clone();
    let radios = tokio::task::spawn_blocking(move || store.list_saved_radios()).await??;
    Ok(Json(radios))
}

#[utoipa::path(
    post, path = "/api/saved-radios",
    request_body = SaveRadioRequest,
    responses(
        (status = 200, description = "Radio saved, or its label updated", body = CreatedRowId),
        (status = 400, description = "Blank device id or label", body = ApiError),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn save_radio(
    State(state): State<AppState>,
    Json(req): Json<SaveRadioRequest>,
) -> Result<Json<CreatedRowId>, AppError> {
    let req = SaveRadioRequest {
        device_id: req.device_id.trim().to_string(),
        label: req.label.trim().to_string(),
    };
    if req.device_id.is_empty() || req.label.is_empty() {
        return Err(AppError::bad_request(
            "a saved radio needs a device id and a label",
        ));
    }
    let engine = state.engine.clone();
    let store = state.store.clone();
    let id = tokio::task::spawn_blocking(move || -> Result<i64, AppError> {
        let id = store.save_radio(&req)?;
        engine.emit_scope(StateScope::SavedRadios);
        Ok(id)
    })
    .await??;
    Ok(Json(CreatedRowId { id }))
}

#[utoipa::path(
    delete, path = "/api/saved-radios/{id}",
    params(("id" = i64, Path, description = "Saved radio id")),
    responses(
        (status = 204, description = "Saved radio removed"),
        (status = 400, description = "Invalid path parameter", body = ApiError),
        (status = 404, description = "Saved radio not found", body = ApiError),
    ),
)]
pub(super) async fn delete_saved_radio(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let engine = state.engine.clone();
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        store.delete_saved_radio(id)?;
        engine.emit_scope(StateScope::SavedRadios);
        Ok(())
    })
    .await??;
    Ok(StatusCode::NO_CONTENT)
}

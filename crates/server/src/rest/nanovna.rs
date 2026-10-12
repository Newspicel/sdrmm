use sdrmm_wire::{
    NanoVnaAntenna, NanoVnaAntennaRequest, NanoVnaLibraryResponse, NanoVnaMeasurement,
    NanoVnaMeasurementInfo, SaveNanoVnaMeasurementRequest, UpdateNanoVnaMeasurementRequest,
};

use super::*;

const MEASUREMENT_BODY_LIMIT: usize = 8 * 1024 * 1024;

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_nanovna_library))
        .routes(routes!(create_nanovna_antenna))
        .routes(routes!(update_nanovna_antenna, delete_nanovna_antenna))
        .merge(
            OpenApiRouter::new()
                .routes(routes!(save_nanovna_measurement))
                .layer(DefaultBodyLimit::max(MEASUREMENT_BODY_LIMIT)),
        )
        .routes(routes!(
            get_nanovna_measurement,
            update_nanovna_measurement,
            delete_nanovna_measurement
        ))
}

#[utoipa::path(
    get, path = "/api/nanovna/library",
    responses((status = 200, description = "Saved antennas and their measurements, without points", body = NanoVnaLibraryResponse)),
)]
pub(super) async fn get_nanovna_library(
    State(state): State<AppState>,
) -> Result<Json<NanoVnaLibraryResponse>, AppError> {
    let store = state.store.clone();
    let library = tokio::task::spawn_blocking(move || -> Result<_, AppError> {
        Ok(NanoVnaLibraryResponse {
            antennas: store.list_nanovna_antennas()?,
            measurements: store.list_nanovna_measurements()?,
        })
    })
    .await??;
    Ok(Json(library))
}

#[utoipa::path(
    post, path = "/api/nanovna/antennas",
    request_body = NanoVnaAntennaRequest,
    responses(
        (status = 200, description = "Antenna stored", body = NanoVnaAntenna),
        (status = 400, description = "Unusable field", body = ApiError),
        (status = 409, description = "That name is taken", body = ApiError),
    ),
)]
pub(super) async fn create_nanovna_antenna(
    State(state): State<AppState>,
    Json(req): Json<NanoVnaAntennaRequest>,
) -> Result<Json<NanoVnaAntenna>, AppError> {
    let store = state.store.clone();
    let antenna = tokio::task::spawn_blocking(move || store.create_nanovna_antenna(&req)).await??;
    Ok(Json(antenna))
}

#[utoipa::path(
    patch, path = "/api/nanovna/antennas/{id}",
    params(("id" = i64, Path, description = "Antenna id")),
    request_body = NanoVnaAntennaRequest,
    responses(
        (status = 200, description = "Antenna updated", body = NanoVnaAntenna),
        (status = 400, description = "Unusable field", body = ApiError),
        (status = 404, description = "Antenna not found", body = ApiError),
        (status = 409, description = "That name is taken", body = ApiError),
    ),
)]
pub(super) async fn update_nanovna_antenna(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<NanoVnaAntennaRequest>,
) -> Result<Json<NanoVnaAntenna>, AppError> {
    let store = state.store.clone();
    let antenna =
        tokio::task::spawn_blocking(move || store.update_nanovna_antenna(id, &req)).await??;
    Ok(Json(antenna))
}

#[utoipa::path(
    delete, path = "/api/nanovna/antennas/{id}",
    params(("id" = i64, Path, description = "Antenna id")),
    responses(
        (status = 204, description = "Antenna and its measurements removed"),
        (status = 404, description = "Antenna not found", body = ApiError),
    ),
)]
pub(super) async fn delete_nanovna_antenna(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || store.delete_nanovna_antenna(id)).await??;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/api/nanovna/antennas/{id}/measurements",
    params(("id" = i64, Path, description = "Antenna id")),
    request_body = SaveNanoVnaMeasurementRequest,
    responses(
        (status = 200, description = "Measurement stored", body = NanoVnaMeasurementInfo),
        (status = 400, description = "Unusable field or points", body = ApiError),
        (status = 404, description = "Antenna not found", body = ApiError),
    ),
)]
pub(super) async fn save_nanovna_measurement(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<SaveNanoVnaMeasurementRequest>,
) -> Result<Json<NanoVnaMeasurementInfo>, AppError> {
    let store = state.store.clone();
    let info =
        tokio::task::spawn_blocking(move || store.save_nanovna_measurement(id, &req)).await??;
    Ok(Json(info))
}

#[utoipa::path(
    get, path = "/api/nanovna/measurements/{id}",
    params(("id" = i64, Path, description = "Measurement id")),
    responses(
        (status = 200, description = "The measurement with its points", body = NanoVnaMeasurement),
        (status = 404, description = "Measurement not found", body = ApiError),
    ),
)]
pub(super) async fn get_nanovna_measurement(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<NanoVnaMeasurement>, AppError> {
    let store = state.store.clone();
    let measurement = tokio::task::spawn_blocking(move || store.nanovna_measurement(id)).await??;
    Ok(Json(measurement))
}

#[utoipa::path(
    patch, path = "/api/nanovna/measurements/{id}",
    params(("id" = i64, Path, description = "Measurement id")),
    request_body = UpdateNanoVnaMeasurementRequest,
    responses(
        (status = 200, description = "Measurement updated", body = NanoVnaMeasurementInfo),
        (status = 400, description = "Unusable field", body = ApiError),
        (status = 404, description = "Measurement not found", body = ApiError),
    ),
)]
pub(super) async fn update_nanovna_measurement(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateNanoVnaMeasurementRequest>,
) -> Result<Json<NanoVnaMeasurementInfo>, AppError> {
    let store = state.store.clone();
    let info =
        tokio::task::spawn_blocking(move || store.update_nanovna_measurement(id, &req)).await??;
    Ok(Json(info))
}

#[utoipa::path(
    delete, path = "/api/nanovna/measurements/{id}",
    params(("id" = i64, Path, description = "Measurement id")),
    responses(
        (status = 204, description = "Measurement removed"),
        (status = 404, description = "Measurement not found", body = ApiError),
    ),
)]
pub(super) async fn delete_nanovna_measurement(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || store.delete_nanovna_measurement(id)).await??;
    Ok(StatusCode::NO_CONTENT)
}

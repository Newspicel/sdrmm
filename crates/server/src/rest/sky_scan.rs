use sdrmm_wire::{SkyScanStatus, SurveyRequest};

use super::{
    ApiError, AppError, AppState, ErrorCode, Json, OpenApiRouter, Path, State, StatusCode, routes,
};
use crate::sky_scans::SkyScanRefusal;

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(get_sky_scan, control_sky_scan))
}

fn conflict(message: &str) -> AppError {
    AppError::new(
        StatusCode::CONFLICT,
        ErrorCode::Conflict,
        message.to_owned(),
    )
}

impl From<SkyScanRefusal> for AppError {
    fn from(refusal: SkyScanRefusal) -> Self {
        match refusal {
            SkyScanRefusal::Missing(node) => Self::not_found(format!("no sky scan {node}")),
            SkyScanRefusal::Invalid(problem) => conflict(problem),
            SkyScanRefusal::NoRadio => conflict("Wire a lane"),
            SkyScanRefusal::NoRotator => conflict("Wire a rotator"),
            SkyScanRefusal::NotTracking => conflict("Set the rotator to Track"),
        }
    }
}

#[utoipa::path(
    get, path = "/api/sky-scan/{node}",
    params(("node" = String, Path, description = "Sky scan node id")),
    responses(
        (status = 200, description = "Every measured point", body = SkyScanStatus),
        (status = 404, description = "No sky scan with that id", body = ApiError),
    ),
)]
pub(super) async fn get_sky_scan(
    State(state): State<AppState>,
    Path(node): Path<String>,
) -> Result<Json<SkyScanStatus>, AppError> {
    state
        .sky_scans
        .status(&node)
        .map(Json)
        .ok_or_else(|| SkyScanRefusal::Missing(node).into())
}

#[utoipa::path(
    post, path = "/api/sky-scan/{node}",
    params(("node" = String, Path, description = "Sky scan node id")),
    request_body = SurveyRequest,
    responses(
        (status = 200, description = "The scan after the action", body = SkyScanStatus),
        (status = 404, description = "No sky scan with that id", body = ApiError),
        (status = 409, description = "The scan cannot do that now", body = ApiError),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn control_sky_scan(
    State(state): State<AppState>,
    Path(node): Path<String>,
    Json(request): Json<SurveyRequest>,
) -> Result<Json<SkyScanStatus>, AppError> {
    state
        .sky_scans
        .act(&state, &node, request.action)
        .map(Json)
        .map_err(Into::into)
}

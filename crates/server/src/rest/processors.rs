use sdrmm_wire::ProcessorReading;

use super::*;

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(get_processor))
}

#[utoipa::path(
    get, path = "/api/processors/{node}",
    params(("node" = String, Path, description = "Array processor node id")),
    responses(
        (status = 200, description = "The reading the processor shows now", body = ProcessorReading),
        (status = 404, description = "No reading from that node", body = ApiError),
    ),
)]
pub(super) async fn get_processor(
    State(state): State<AppState>,
    Path(node): Path<String>,
) -> Result<Json<ProcessorReading>, AppError> {
    state
        .reports
        .latest(&node)
        .map(Json)
        .ok_or_else(|| AppError::not_found(format!("No processor {node}")))
}

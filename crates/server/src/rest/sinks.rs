use super::*;

#[utoipa::path(
    delete, path = "/api/sinks/{node}",
    params(("node" = String, Path, description = "Map, Readout, Decoder log or Propagation node")),
    responses(
        (status = 204, description = "What the node held is gone; new records still arrive"),
        (status = 404, description = "The node keeps no sink feed", body = ApiError),
    ),
)]
pub(super) async fn clear_sink(
    State(state): State<AppState>,
    Path(node): Path<String>,
) -> Result<StatusCode, AppError> {
    if state.sinks.clear(&node) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found(format!("{node} keeps no sink feed")))
    }
}

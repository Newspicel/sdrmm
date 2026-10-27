use sdrmm_wire::{
    MAX_NODES, PatchError, PatchGraph,
    patch::resolve::{self, ResolveCtx, WireVerdict, WiringRequest},
};

use super::*;

#[utoipa::path(
    post, path = "/api/patch/wiring",
    request_body = WiringRequest,
    responses(
        (
            status = 200,
            description = "For a wire dragged from `from`, every input of that port type in \
                           `graph` and why it would refuse the wire. Counts live radio clocks; \
                           the save stays the authority",
            body = Vec<WireVerdict>,
        ),
        (status = 400, description = "The graph is larger than a workspace may hold", body = ApiError),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn post_patch_wiring(
    State(state): State<AppState>,
    Json(request): Json<WiringRequest>,
) -> Result<Json<Vec<WireVerdict>>, AppError> {
    holdable(&request.graph)?;
    let engine = state.engine.clone();
    let verdicts = tokio::task::spawn_blocking(move || {
        let live = engine.snapshot();
        let ctx = ResolveCtx {
            descriptors: Some(sdrmm_channels::descriptor_table()),
            live: Some(&live),
        };
        resolve::wiring(&request.graph, &request.from, &ctx)
    })
    .await?;
    Ok(Json(verdicts))
}

fn holdable(graph: &PatchGraph) -> Result<(), AppError> {
    if graph.nodes.len() <= MAX_NODES {
        return Ok(());
    }
    Err(AppError::bad_request(PatchError::TooManyNodes.to_string()))
}

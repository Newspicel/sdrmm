use sdrmm_wire::{ControlAck, ControlCommand, NodeAction, SweepJob};

use super::*;

#[utoipa::path(
    post, path = "/api/workspaces/active/nodes/{node}/actions",
    params(("node" = String, Path, description = "Node of the active workspace")),
    request_body = NodeAction,
    responses(
        (status = 200, description = "One ack per wired target; a failed target carries `error`",
         body = Vec<ControlAck>),
        (status = 400, description = "The node takes no such action or controls nothing",
         body = ApiError),
        (status = 404, description = "No such node", body = ApiError),
    ),
)]
pub(super) async fn node_action(
    State(state): State<AppState>,
    Path(node): Path<String>,
    Json(action): Json<NodeAction>,
) -> Result<Json<Vec<ControlAck>>, AppError> {
    let acks = tokio::task::spawn_blocking(move || act(&state, &node, action)).await??;
    Ok(Json(acks))
}

pub(crate) fn act(
    state: &AppState,
    node: &str,
    action: NodeAction,
) -> Result<Vec<ControlAck>, AppError> {
    let resolved = state.resolved.current();
    let body = &resolved
        .graph
        .node(node)
        .ok_or_else(|| AppError::not_found(format!("No node {node}")))?
        .body;
    let command = match (action, body) {
        (NodeAction::Scan(action), NodeBody::Scanner(scanner)) => {
            ControlCommand::Sweep(SweepJob::Scan {
                settings: scanner.settings.clone(),
                action,
            })
        }
        (NodeAction::Hunt(action), NodeBody::Hunt(hunt)) => ControlCommand::Sweep(SweepJob::Hunt {
            params: hunt.sweep,
            action,
        }),
        _ => {
            return Err(AppError::bad_request(format!(
                "{node} takes no such action"
            )));
        }
    };
    let acks = state
        .control
        .deliver(&state.engine, &resolved.patch, node, &command);
    if acks.is_empty() {
        return Err(AppError::bad_request("Wire control to a decoder"));
    }
    Ok(acks)
}

pub(crate) fn succeeded(acks: Vec<ControlAck>) -> Result<Vec<ControlAck>, AppError> {
    match acks.iter().find_map(|ack| ack.error.as_ref()) {
        Some(error) => Err(AppError::bad_request(error.clone())),
        None => Ok(acks),
    }
}

use sdrmm_wire::{ControlAck, ControlCommand, NodeActed, NodeAction, SweepJob};

use super::*;

mod capture;
mod retype;
mod tune;

pub(crate) use tune::{Tuner, tune as tune_node};

#[utoipa::path(
    post, path = "/api/workspaces/active/nodes/{node}/actions",
    params(("node" = String, Path, description = "Node of the active workspace")),
    request_body = NodeAction,
    responses(
        (status = 200, description = "What the action did. A control action lists one ack per \
                                      wired target; a failed target carries `error`",
         body = NodeActed),
        (status = 400, description = "The node takes no such action, is locked, or is not wired",
         body = ApiError),
        (status = 404, description = "No such node", body = ApiError),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn node_action(
    State(state): State<AppState>,
    Path(node): Path<String>,
    author: Author,
    Json(action): Json<NodeAction>,
) -> Result<Json<NodeActed>, AppError> {
    let acted = tokio::task::spawn_blocking(move || act(&state, &node, action, author.as_deref()))
        .await??;
    Ok(Json(acted))
}

pub(crate) fn act(
    state: &AppState,
    node: &str,
    action: NodeAction,
    author: Option<&str>,
) -> Result<NodeActed, AppError> {
    match action {
        NodeAction::Tune { freq_hz } => {
            let tuner = Tuner {
                author,
                actor: None,
            };
            tune_node(state, node, freq_hz, tuner).map(|()| NodeActed::Tuned)
        }
        NodeAction::Retype { channel_type } => retype::retype(state, node, &channel_type, author)
            .map(|dropped| NodeActed::Retyped { dropped }),
        NodeAction::Playback(request) => {
            capture::playback(state, node, &request).map(NodeActed::Playback)
        }
        NodeAction::NetworkExport(action) => {
            capture::network_export(state, node, action).map(NodeActed::NetworkExport)
        }
        NodeAction::Scan(_) | NodeAction::Hunt(_) => {
            control(state, node, action).map(NodeActed::Controlled)
        }
    }
}

fn control(state: &AppState, node: &str, action: NodeAction) -> Result<Vec<ControlAck>, AppError> {
    let resolved = state.resolved.current();
    let body = &node_of(&resolved, node)?.body;
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
        _ => return Err(takes_no_such_action(node)),
    };
    let acks = state
        .control
        .deliver(&state.engine, &resolved.patch, node, &command);
    if acks.is_empty() {
        return Err(AppError::bad_request("Wire control to a decoder"));
    }
    Ok(acks)
}

fn node_of<'a>(
    resolved: &'a crate::resolved::Resolved,
    node: &str,
) -> Result<&'a sdrmm_wire::PatchNode, AppError> {
    resolved
        .graph
        .node(node)
        .ok_or_else(|| AppError::not_found(format!("No node {node}")))
}

fn takes_no_such_action(node: &str) -> AppError {
    AppError::bad_request(format!("{node} takes no such action"))
}

pub(crate) fn controlled(acted: NodeActed) -> Result<Vec<ControlAck>, AppError> {
    let NodeActed::Controlled(acks) = acted else {
        return Err(AppError::internal(
            "a control action acted on nothing".to_owned(),
        ));
    };
    match acks.iter().find_map(|ack| ack.error.as_ref()) {
        Some(error) => Err(AppError::bad_request(error.clone())),
        None => Ok(acks),
    }
}

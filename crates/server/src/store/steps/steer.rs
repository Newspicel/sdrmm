use sdrmm_wire::{BEARING_PORT, EVENTS_PORT, NodeKind, STEER_PORT, WorkspaceNoticeKind};
use serde_json::Value;

use super::{graph_nodes, wires};
use crate::store::node_kind;

pub(super) fn steer_from_bearing(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    let finders: Vec<String> = graph_nodes(snapshot)
        .filter(|node| node_kind(node) == Some(NodeKind::Df.as_str()))
        .filter_map(|node| Some(node.get("id")?.as_str()?.to_owned()))
        .collect();
    for edge in wires::graph_edges(snapshot).into_iter().flatten() {
        let (Some(from), Some(to)) = (wires::end(edge, "from"), wires::end(edge, "to")) else {
            continue;
        };
        if from.port == EVENTS_PORT && to.port == STEER_PORT && finders.contains(&from.node) {
            edge["from"]["port"] = Value::from(BEARING_PORT);
        }
    }
    Vec::new()
}

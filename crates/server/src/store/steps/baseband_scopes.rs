use std::collections::HashSet;

use sdrmm_wire::{DroppedNode, WorkspaceNoticeKind};
use serde_json::Value;

use super::graph_nodes;
use crate::store::{node_kind, remove_nodes};

pub(in crate::store) const RETIRED_BASEBAND_SCOPE: &str = "baseband_scope";

/// The inspector draws the baseband of whichever node it shows, so the node that only drew one
/// is gone, and so are its wires.
pub(super) fn drop_baseband_scopes(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    let nodes: Vec<DroppedNode> = graph_nodes(snapshot)
        .filter(|node| node_kind(node) == Some(RETIRED_BASEBAND_SCOPE))
        .map(|node| DroppedNode {
            id: text(node, "id").unwrap_or_default(),
            kind: RETIRED_BASEBAND_SCOPE.to_owned(),
            label: text(node, "label"),
        })
        .collect();
    if nodes.is_empty() {
        return Vec::new();
    }
    let ids: HashSet<String> = nodes.iter().map(|node| node.id.clone()).collect();
    remove_nodes(snapshot, &ids);
    if let Some(nodes) = snapshot
        .get_mut("graph")
        .and_then(|graph| graph.get_mut("nodes"))
        .and_then(Value::as_array_mut)
    {
        nodes.retain(|node| node_kind(node) != Some(RETIRED_BASEBAND_SCOPE));
    }
    vec![WorkspaceNoticeKind::DroppedNodes { nodes }]
}

fn text(node: &Value, key: &str) -> Option<String> {
    node.get(key).and_then(Value::as_str).map(str::to_owned)
}

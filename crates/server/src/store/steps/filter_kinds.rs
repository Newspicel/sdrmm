use sdrmm_wire::{EventKind, NodeKind, WorkspaceNoticeKind};
use serde_json::Value;

use super::graph_nodes;
use crate::store::node_kind;

pub(super) fn drop_unknown_filter_kinds(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    graph_nodes(snapshot)
        .filter(|node| node_kind(node) == Some(NodeKind::EventFilter.as_str()))
        .filter_map(drop_unknown)
        .collect()
}

fn drop_unknown(node: &mut Value) -> Option<WorkspaceNoticeKind> {
    let id = node.get("id").and_then(Value::as_str)?.to_owned();
    let kinds = node.get_mut("data")?.get_mut("kinds")?.as_array_mut()?;
    let mut dropped = Vec::new();
    kinds.retain(|kind| {
        let known = kind.as_str().and_then(EventKind::parse).is_some();
        if !known {
            dropped.push(
                kind.as_str()
                    .map_or_else(|| kind.to_string(), str::to_owned),
            );
        }
        known
    });
    (!dropped.is_empty()).then_some(WorkspaceNoticeKind::DroppedFilterKinds {
        node: id,
        kinds: dropped,
    })
}

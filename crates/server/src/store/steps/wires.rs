use std::collections::HashMap;

use sdrmm_wire::{EVENTS_PORT, NodeKind, NoticeWire, PortRef, WorkspaceNoticeKind};
use serde_json::Value;

use super::graph_nodes;
use crate::store::node_kind;

pub(super) fn graph_edges(snapshot: &mut Value) -> Option<&mut Vec<Value>> {
    snapshot.get_mut("graph")?.get_mut("edges")?.as_array_mut()
}

pub(super) fn end(edge: &Value, side: &str) -> Option<PortRef> {
    let end = edge.get(side)?;
    Some(PortRef {
        node: end.get("node")?.as_str()?.to_owned(),
        port: end.get("port")?.as_str()?.to_owned(),
    })
}

pub(super) fn drop_where(
    snapshot: &mut Value,
    mut refusal: impl FnMut(&Value) -> Option<String>,
) -> Vec<WorkspaceNoticeKind> {
    let Some(edges) = graph_edges(snapshot) else {
        return Vec::new();
    };
    let mut dropped = Vec::new();
    edges.retain(|edge| {
        let Some(reason) = refusal(edge) else {
            return true;
        };
        if let (Some(from), Some(to)) = (end(edge, "from"), end(edge, "to")) {
            dropped.push(NoticeWire { from, to, reason });
        }
        false
    });
    if dropped.is_empty() {
        Vec::new()
    } else {
        vec![WorkspaceNoticeKind::DroppedWires { wires: dropped }]
    }
}

pub(super) fn ends(edge: &Value) -> Option<(PortRef, PortRef)> {
    Some((end(edge, "from")?, end(edge, "to")?))
}

pub(super) fn kinds_of(snapshot: &mut Value) -> HashMap<String, String> {
    graph_nodes(snapshot)
        .filter_map(|node| {
            Some((
                node.get("id")?.as_str()?.to_owned(),
                node_kind(node)?.to_owned(),
            ))
        })
        .collect()
}

pub(super) fn event_sinks(
    edges: &[Value],
    kinds: &HashMap<String, String>,
    source: &str,
    wanted: &[NodeKind],
) -> Vec<String> {
    let kind_of = |node: &str| kinds.get(node).and_then(|kind| NodeKind::parse(kind));
    let mut frontier = vec![source.to_owned()];
    let mut seen = vec![source.to_owned()];
    let mut sinks = Vec::new();
    while let Some(node) = frontier.pop() {
        for (from, to) in edges.iter().filter_map(ends) {
            if from.node != node || from.port != EVENTS_PORT || to.port != EVENTS_PORT {
                continue;
            }
            match kind_of(&to.node) {
                Some(NodeKind::EventFilter) if !seen.contains(&to.node) => {
                    seen.push(to.node.clone());
                    frontier.push(to.node);
                }
                Some(kind) if wanted.contains(&kind) && !sinks.contains(&to.node) => {
                    sinks.push(to.node);
                }
                _ => {}
            }
        }
    }
    sinks
}

use sdrmm_wire::{CONTROL_PORT, IQ_PORT, NodeKind, NoticeWire, PortRef, WorkspaceNoticeKind};
use serde_json::{Value, json};

use super::{graph_nodes, wires};
use crate::store::node_kind;

const WATCHED: &str = "The lane it watches";

pub(super) fn wire_watched_lanes(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    let watchers: Vec<String> = graph_nodes(snapshot)
        .filter(|node| {
            node_kind(node).is_some_and(|kind| {
                kind == NodeKind::Scanner.as_str() || kind == NodeKind::Hunt.as_str()
            })
        })
        .filter_map(|node| Some(node.get("id")?.as_str()?.to_owned()))
        .collect();
    let Some(edges) = wires::graph_edges(snapshot) else {
        return Vec::new();
    };
    let mut added = Vec::new();
    for watcher in watchers {
        let input = PortRef {
            node: watcher,
            port: IQ_PORT.to_owned(),
        };
        if let Some(lane) = watched_lane(edges, &input) {
            edges.push(json!({ "from": lane, "to": input }));
            added.push(NoticeWire {
                from: lane,
                to: input,
                reason: WATCHED.to_owned(),
            });
        }
    }
    if added.is_empty() {
        Vec::new()
    } else {
        vec![WorkspaceNoticeKind::AddedWires { wires: added }]
    }
}

fn watched_lane(edges: &[Value], input: &PortRef) -> Option<PortRef> {
    let ends = |edge: &Value| Some((wires::end(edge, "from")?, wires::end(edge, "to")?));
    if edges.iter().filter_map(ends).any(|(_, to)| to == *input) {
        return None;
    }
    let driven = edges.iter().filter_map(ends).find_map(|(from, to)| {
        (from.node == input.node && from.port == CONTROL_PORT).then_some(to.node)
    })?;
    edges
        .iter()
        .filter_map(ends)
        .find_map(|(from, to)| (to.node == driven && to.port == IQ_PORT).then_some(from))
}

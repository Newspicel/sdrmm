use sdrmm_wire::{NoticeWire, PortRef, WorkspaceNoticeKind};
use serde_json::Value;

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

use sdrmm_wire::{EVENTS_PORT, NodeKind, WorkspaceNoticeKind};
use serde_json::Value;

use super::{graph_nodes, wires};
use crate::store::node_kind;

const SENDS_NO_EVENTS: &str = "Sends no events";

pub(super) fn drop_silent_event_wires(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    let silent: Vec<String> = graph_nodes(snapshot)
        .filter(|node| sends_no_events(node))
        .filter_map(|node| Some(node.get("id")?.as_str()?.to_owned()))
        .collect();
    wires::drop_where(snapshot, |edge| {
        let from = wires::end(edge, "from")?;
        (from.port == EVENTS_PORT && silent.contains(&from.node))
            .then(|| SENDS_NO_EVENTS.to_owned())
    })
}

fn sends_no_events(node: &Value) -> bool {
    node_kind(node) == Some(NodeKind::Channel.as_str())
        && node
            .get("data")
            .and_then(|data| data.get("channel_type"))
            .and_then(Value::as_str)
            .and_then(sdrmm_channels::descriptor)
            .is_some_and(|descriptor| descriptor.emits.is_empty() && descriptor.calls.is_none())
}

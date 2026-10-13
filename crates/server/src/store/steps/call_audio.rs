use sdrmm_wire::{AUDIO_PORT, EventOutputNode, NodeKind, NoticeWire, PortRef, WorkspaceNoticeKind};
use serde_json::{Value, json};

use super::{graph_nodes, wires};
use crate::store::node_kind;

const KEEPS_AUDIO: &str = "Keeps call audio";
const RECORD_AUDIO: &str = "record_audio";
const CLIP_SINKS: &[NodeKind] = &[NodeKind::DecoderLog, NodeKind::EventOutput];

pub(super) fn wire_call_audio(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    let kinds = wires::kinds_of(snapshot);
    let sources: Vec<String> = graph_nodes(snapshot)
        .filter(|node| keeps_call_audio(node))
        .filter_map(|node| Some(node.get("id")?.as_str()?.to_owned()))
        .collect();
    let silent: Vec<String> = graph_nodes(snapshot)
        .filter(|node| drops_audio(node))
        .filter_map(|node| Some(node.get("id")?.as_str()?.to_owned()))
        .collect();
    drop_audio_switch(snapshot);
    let Some(edges) = wires::graph_edges(snapshot) else {
        return Vec::new();
    };
    let mut added = Vec::new();
    for source in sources {
        for sink in wires::event_sinks(edges, &kinds, &source, CLIP_SINKS)
            .into_iter()
            .filter(|sink| !silent.contains(sink))
        {
            let from = PortRef {
                node: source.clone(),
                port: AUDIO_PORT.to_owned(),
            };
            let to = PortRef {
                node: sink,
                port: AUDIO_PORT.to_owned(),
            };
            if wires::has_wire(edges, &from, &to) {
                continue;
            }
            edges.push(json!({ "from": from, "to": to }));
            added.push(NoticeWire {
                from,
                to,
                reason: KEEPS_AUDIO.to_owned(),
            });
        }
    }
    if added.is_empty() {
        Vec::new()
    } else {
        vec![WorkspaceNoticeKind::AddedWires { wires: added }]
    }
}

fn keeps_call_audio(node: &Value) -> bool {
    match node_kind(node) {
        Some(kind) if kind == NodeKind::DmrTrunk.as_str() => true,
        Some(kind) if kind == NodeKind::SpectrumMonitor.as_str() => node
            .get("data")
            .and_then(|data| data.get(RECORD_AUDIO))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        Some(kind) if kind == NodeKind::Channel.as_str() => node
            .get("data")
            .and_then(|data| data.get("channel_type"))
            .and_then(Value::as_str)
            .and_then(sdrmm_channels::descriptor)
            .is_some_and(|descriptor| descriptor.calls.is_some() && descriptor.has_audio),
        _ => false,
    }
}

fn drops_audio(node: &Value) -> bool {
    node_kind(node) == Some(NodeKind::EventOutput.as_str())
        && node
            .get("data")
            .cloned()
            .and_then(|data| serde_json::from_value::<EventOutputNode>(data).ok())
            .is_none_or(|output| !output.target.carries_audio())
}

fn drop_audio_switch(snapshot: &mut Value) {
    for node in graph_nodes(snapshot) {
        if node_kind(node) != Some(NodeKind::SpectrumMonitor.as_str()) {
            continue;
        }
        if let Some(data) = node.get_mut("data").and_then(Value::as_object_mut) {
            data.remove(RECORD_AUDIO);
        }
    }
}

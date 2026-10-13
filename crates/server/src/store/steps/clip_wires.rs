use sdrmm_wire::{AUDIO_PORT, NodeKind, WorkspaceNoticeKind};
use serde_json::Value;

use super::{graph_nodes, wires};
use crate::store::node_kind;

const CLIPS_RIDE_ON_EVENTS: &str = "Clips ride on the events wire";
const RECORD_AUDIO: &str = "record_audio";
const CLIP_ENDS: &[NodeKind] = &[
    NodeKind::DecoderLog,
    NodeKind::EventOutput,
    NodeKind::SpectrumMonitor,
];

pub(super) fn drop_clip_wires(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    drop_audio_switch(snapshot);
    let ends: Vec<String> = graph_nodes(snapshot)
        .filter(|node| {
            CLIP_ENDS
                .iter()
                .any(|kind| node_kind(node) == Some(kind.as_str()))
        })
        .filter_map(|node| Some(node.get("id")?.as_str()?.to_owned()))
        .collect();
    wires::drop_where(snapshot, |edge| {
        let (from, to) = wires::ends(edge)?;
        let clip_end =
            |end: &sdrmm_wire::PortRef| end.port == AUDIO_PORT && ends.contains(&end.node);
        (clip_end(&from) || clip_end(&to)).then(|| CLIPS_RIDE_ON_EVENTS.to_owned())
    })
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

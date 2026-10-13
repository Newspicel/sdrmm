use sdrmm_wire::{DroppedNode, NodeKind, NoticeWire, PortRef, VIDEO_PORT, WorkspaceNoticeKind};
use serde_json::{Value, json};

use super::{graph_nodes, wires};
use crate::store::node_kind;

const KEEPS_PICTURES: &str = "Keeps its pictures";
const BESIDE_PX: f64 = 600.0;

struct Gallery {
    channel: String,
    readout: String,
}

pub(super) fn wire_pictures(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    let kinds = wires::kinds_of(snapshot);
    let channels: Vec<String> = graph_nodes(snapshot)
        .filter(|node| has_video(node))
        .filter_map(|node| Some(node.get("id")?.as_str()?.to_owned()))
        .collect();
    let Some(edges) = wires::graph_edges(snapshot) else {
        return Vec::new();
    };
    let galleries: Vec<Gallery> = channels
        .into_iter()
        .filter(|channel| !shows_video(edges, &kinds, channel))
        .filter_map(|channel| {
            let readout = wires::event_sinks(edges, &kinds, &channel, &[NodeKind::Readout])
                .into_iter()
                .next()?;
            Some(Gallery { channel, readout })
        })
        .collect();
    if galleries.is_empty() {
        return Vec::new();
    }
    let mut nodes = Vec::new();
    let mut added = Vec::new();
    for gallery in galleries {
        let id = free_id(snapshot, &format!("{}-pictures", gallery.channel));
        let position = beside(snapshot, &gallery.readout);
        if let Some(list) = snapshot
            .get_mut("graph")
            .and_then(|graph| graph.get_mut("nodes"))
            .and_then(Value::as_array_mut)
        {
            list.push(json!({ "id": id, "kind": NodeKind::Video.as_str(), "position": position }));
        }
        let from = PortRef {
            node: gallery.channel,
            port: VIDEO_PORT.to_owned(),
        };
        let to = PortRef {
            node: id.clone(),
            port: VIDEO_PORT.to_owned(),
        };
        if let Some(edges) = wires::graph_edges(snapshot) {
            edges.push(json!({ "from": from, "to": to }));
        }
        nodes.push(DroppedNode {
            id,
            kind: NodeKind::Video.as_str().to_owned(),
            label: None,
        });
        added.push(NoticeWire {
            from,
            to,
            reason: KEEPS_PICTURES.to_owned(),
        });
    }
    vec![
        WorkspaceNoticeKind::AddedNodes { nodes },
        WorkspaceNoticeKind::AddedWires { wires: added },
    ]
}

fn has_video(node: &Value) -> bool {
    node_kind(node) == Some(NodeKind::Channel.as_str())
        && node
            .get("data")
            .and_then(|data| data.get("channel_type"))
            .and_then(Value::as_str)
            .and_then(sdrmm_channels::descriptor)
            .is_some_and(|descriptor| descriptor.has_video)
}

fn shows_video(
    edges: &[Value],
    kinds: &std::collections::HashMap<String, String>,
    channel: &str,
) -> bool {
    edges.iter().filter_map(wires::ends).any(|(from, to)| {
        from.node == channel
            && from.port == VIDEO_PORT
            && kinds.get(&to.node).map(String::as_str) == Some(NodeKind::Video.as_str())
    })
}

fn free_id(snapshot: &mut Value, wanted: &str) -> String {
    let taken: Vec<String> = graph_nodes(snapshot)
        .filter_map(|node| Some(node.get("id")?.as_str()?.to_owned()))
        .collect();
    (1..)
        .map(|n| {
            if n == 1 {
                wanted.to_owned()
            } else {
                format!("{wanted}-{n}")
            }
        })
        .find(|id| !taken.contains(id))
        .unwrap_or_else(|| wanted.to_owned())
}

fn beside(snapshot: &mut Value, readout: &str) -> Value {
    let at = graph_nodes(snapshot)
        .find(|node| node.get("id").and_then(Value::as_str) == Some(readout))
        .and_then(|node| node.get("position").cloned())
        .unwrap_or_else(|| json!({ "x": 0.0, "y": 0.0 }));
    let x = at.get("x").and_then(Value::as_f64).unwrap_or_default() + BESIDE_PX;
    let y = at.get("y").and_then(Value::as_f64).unwrap_or_default();
    json!({ "x": x, "y": y })
}

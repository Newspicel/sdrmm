use sdrmm_wire::{DroppedNode, NodeKind, NoticeWire, PortRef, VIDEO_PORT, WorkspaceNoticeKind};
use serde_json::{Value, json};

use super::{
    graph_nodes,
    place::{beside, free_id, push_node},
    wires,
};
use crate::store::node_kind;

const KEEPS_PICTURES: &str = "Keeps its pictures";

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
        push_node(
            snapshot,
            json!({ "id": id, "kind": NodeKind::Video.as_str(), "position": position }),
        );
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

use sdrmm_wire::{
    ARRAY_LANE_PORT, DroppedNode, IQ_PORT, NodeKind, NoticeWire, PortRef, WorkspaceNoticeKind,
    array::MIN_ARRAY_LANES, port_stream, stream_port,
};
use serde_json::{Value, json};

use super::{
    graph_nodes,
    place::{below, beside, free_id, push_node},
    wires,
};
use crate::store::node_kind;

const BANK_BACKEND: &str = "kraken";
const DONGLE_BACKEND: &str = "rtlsdr";
const FIRST_SERIAL: u32 = 1000;
const BANK_LANES: u32 = 5;
const ONE_ANTENNA: &str = "One dongle per antenna";

#[derive(Default)]
struct Added {
    nodes: Vec<DroppedNode>,
    wires: Vec<NoticeWire>,
}

impl Added {
    fn node(&mut self, id: &str, kind: NodeKind) {
        self.nodes.push(DroppedNode {
            id: id.to_owned(),
            kind: kind.as_str().to_owned(),
            label: None,
        });
    }

    fn notices(self) -> Vec<WorkspaceNoticeKind> {
        let mut notices = Vec::new();
        if !self.nodes.is_empty() {
            notices.push(WorkspaceNoticeKind::AddedNodes { nodes: self.nodes });
        }
        if !self.wires.is_empty() {
            notices.push(WorkspaceNoticeKind::AddedWires { wires: self.wires });
        }
        notices
    }
}

pub(super) fn split_kraken_banks(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    let mut added = Added::default();
    for bank in banks(snapshot) {
        split_bank(snapshot, &bank, &mut added);
    }
    added.notices()
}

fn banks(snapshot: &mut Value) -> Vec<String> {
    graph_nodes(snapshot)
        .filter(|node| is_bank(node))
        .filter_map(|node| Some(node.get("id")?.as_str()?.to_owned()))
        .collect()
}

fn is_bank(node: &Value) -> bool {
    node_kind(node) == Some(NodeKind::Device.as_str())
        && node
            .get("data")
            .and_then(|data| data.get("device"))
            .and_then(|device| device.get("backend"))
            .and_then(Value::as_str)
            == Some(BANK_BACKEND)
}

fn split_bank(snapshot: &mut Value, bank: &str, added: &mut Added) {
    let lanes = lane_count(snapshot, bank);
    take_first_dongle(snapshot, bank);
    let mut ids = vec![bank.to_owned()];
    for lane in 1..lanes {
        ids.push(add_dongle(snapshot, bank, lane, added));
    }
    move_lane_wires(snapshot, &ids);
    if lanes >= MIN_ARRAY_LANES && !reaches_array(snapshot, &ids) {
        add_array(snapshot, bank, &ids, added);
    }
}

fn lane_count(snapshot: &mut Value, bank: &str) -> u32 {
    let Some(edges) = wires::graph_edges(snapshot) else {
        return BANK_LANES;
    };
    edges
        .iter()
        .filter_map(|edge| wires::end(edge, "from"))
        .filter(|from| from.node == bank)
        .filter_map(|from| port_stream(IQ_PORT, &from.port))
        .max()
        .map_or(BANK_LANES, |top| top + 1)
}

fn dongle(lane: u32) -> Value {
    json!({ "backend": DONGLE_BACKEND, "key": (FIRST_SERIAL + lane).to_string() })
}

fn take_first_dongle(snapshot: &mut Value, bank: &str) {
    for node in graph_nodes(snapshot) {
        if node.get("id").and_then(Value::as_str) != Some(bank) {
            continue;
        }
        if let Some(data) = node.get_mut("data").and_then(Value::as_object_mut) {
            data.insert("device".to_owned(), dongle(0));
        }
    }
}

fn add_dongle(snapshot: &mut Value, bank: &str, lane: u32, added: &mut Added) -> String {
    let id = free_id(snapshot, &format!("{bank}-lane{}", lane + 1));
    let position = below(snapshot, bank, lane);
    push_node(
        snapshot,
        json!({
            "id": id,
            "kind": NodeKind::Device.as_str(),
            "position": position,
            "data": { "device": dongle(lane) }
        }),
    );
    added.node(&id, NodeKind::Device);
    id
}

fn move_lane_wires(snapshot: &mut Value, ids: &[String]) {
    let Some(bank) = ids.first().cloned() else {
        return;
    };
    let Some(edges) = wires::graph_edges(snapshot) else {
        return;
    };
    for edge in edges.iter_mut() {
        let Some(from) = wires::end(edge, "from") else {
            continue;
        };
        if from.node != bank {
            continue;
        }
        let Some(lane) = port_stream(IQ_PORT, &from.port).filter(|lane| *lane > 0) else {
            continue;
        };
        let Some(moved) = ids.get(lane as usize).cloned() else {
            continue;
        };
        if let Some(end) = edge.get_mut("from").and_then(Value::as_object_mut) {
            end.insert("node".to_owned(), Value::from(moved));
            end.insert("port".to_owned(), Value::from(IQ_PORT));
        }
    }
}

fn reaches_array(snapshot: &mut Value, ids: &[String]) -> bool {
    let kinds = wires::kinds_of(snapshot);
    let Some(edges) = wires::graph_edges(snapshot) else {
        return false;
    };
    edges.iter().filter_map(wires::ends).any(|(from, to)| {
        from.port == IQ_PORT
            && ids.contains(&from.node)
            && kinds.get(&to.node).map(String::as_str) == Some(NodeKind::Array.as_str())
    })
}

fn add_array(snapshot: &mut Value, bank: &str, ids: &[String], added: &mut Added) {
    let id = free_id(snapshot, &format!("{bank}-array"));
    let position = beside(snapshot, bank);
    push_node(
        snapshot,
        json!({
            "id": id,
            "kind": NodeKind::Array.as_str(),
            "position": position,
            "data": {}
        }),
    );
    added.node(&id, NodeKind::Array);
    for (lane, node) in (0u32..).zip(ids) {
        let from = PortRef {
            node: node.clone(),
            port: IQ_PORT.to_owned(),
        };
        let to = PortRef {
            node: id.clone(),
            port: stream_port(ARRAY_LANE_PORT, lane),
        };
        if let Some(edges) = wires::graph_edges(snapshot) {
            edges.push(json!({ "from": from, "to": to }));
        }
        added.wires.push(NoticeWire {
            from,
            to,
            reason: ONE_ANTENNA.to_owned(),
        });
    }
}

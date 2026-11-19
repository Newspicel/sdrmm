use sdrmm_wire::{NodeKind, RadioUse, TRANSMIT_PORT, WorkspaceNoticeKind};
use serde_json::{Map, Value};

use super::{graph_nodes, wires};

/// A radio a transmitter was wired to sent and received before a radio had a use, so it keeps
/// doing both. Every other radio takes the default, receive.
pub(super) fn keep_transmitting_radios(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    let kinds = wires::kinds_of(snapshot);
    let is = |node: &str, kind: NodeKind| kinds.get(node).is_some_and(|k| k == kind.as_str());
    let keyed: Vec<String> = wires::graph_edges(snapshot)
        .into_iter()
        .flatten()
        .filter_map(|edge| wires::ends(edge))
        .filter(|(from, to)| {
            from.port == TRANSMIT_PORT
                && to.port == TRANSMIT_PORT
                && is(&from.node, NodeKind::Transmit)
                && is(&to.node, NodeKind::Device)
        })
        .map(|(_, to)| to.node)
        .collect();
    let both = serde_json::to_value(RadioUse::Both).unwrap_or(Value::Null);
    for node in graph_nodes(snapshot) {
        let named = node.get("id").and_then(Value::as_str);
        if !named.is_some_and(|id| keyed.iter().any(|radio| radio == id)) {
            continue;
        }
        if let Some(fields) = node.as_object_mut() {
            let data = fields
                .entry("data")
                .or_insert_with(|| Value::Object(Map::new()));
            if let Some(data) = data.as_object_mut() {
                data.entry("radio_use").or_insert_with(|| both.clone());
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use rusqlite::params;
    use sdrmm_wire::{NodeBody, PatchGraph};
    use serde_json::json;

    use super::*;
    use crate::store::Store;

    fn at(id: &str, kind: &str) -> Value {
        json!({ "id": id, "kind": kind, "position": { "x": 0.0, "y": 0.0 }, "data": {} })
    }

    fn keyed() -> Value {
        json!({
            "version": sdrmm_wire::WORKSPACE_SNAPSHOT_VERSION,
            "graph": {
                "nodes": [at("dev", "device"), at("quiet", "device"), at("tx", "transmit")],
                "edges": [{
                    "from": { "node": "tx", "port": "transmit" },
                    "to": { "node": "dev", "port": "transmit" }
                }]
            }
        })
    }

    fn use_of(graph: &PatchGraph, node: &str) -> RadioUse {
        match graph.node(node).map(|found| &found.body) {
            Some(NodeBody::Device(device)) => device.radio_use,
            other => panic!("{node} is no device: {other:?}"),
        }
    }

    fn plant(store: &Store, id: i64, snapshot: &Value) {
        store
            .lock()
            .execute(
                "UPDATE workspaces SET snapshot = ?2 WHERE id = ?1",
                params![id, snapshot.to_string()],
            )
            .expect("plant the old workspace");
    }

    #[test]
    fn only_a_radio_a_transmitter_is_wired_to_sends_and_receives() {
        let mut snapshot = keyed();
        assert!(keep_transmitting_radios(&mut snapshot).is_empty());
        let nodes = &snapshot["graph"]["nodes"];
        assert_eq!(nodes[0]["data"]["radio_use"], "both");
        assert!(nodes[1]["data"].get("radio_use").is_none());
    }

    #[test]
    fn a_stored_workspace_keeps_its_transmitter_radio_once() {
        let file = tempfile::NamedTempFile::new().expect("temp db");
        let id = {
            let store = Store::open(Some(file.path())).expect("open");
            let id = store
                .active_workspace_id()
                .expect("active")
                .expect("seeded");
            store
                .lock()
                .execute(
                    "DELETE FROM server_meta WHERE key = 'step:keep_transmitting_radios'",
                    [],
                )
                .expect("forget the step");
            plant(&store, id, &keyed());
            id
        };
        let store = Store::open(Some(file.path())).expect("reopen");
        let graph = store.workspace(id).expect("opens").snapshot.graph;
        assert_eq!(use_of(&graph, "dev"), RadioUse::Both);
        assert_eq!(use_of(&graph, "quiet"), RadioUse::Receive);

        plant(&store, id, &keyed());
        drop(store);
        let again = Store::open(Some(file.path())).expect("reopen again");
        let graph = again.workspace(id).expect("opens").snapshot.graph;
        assert_eq!(
            use_of(&graph, "dev"),
            RadioUse::Receive,
            "a radio set back to receive stays there"
        );
    }
}

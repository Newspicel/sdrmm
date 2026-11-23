use sdrmm_wire::{NodeKind, WorkspaceNoticeKind};
use serde_json::{Map, Value, json};

use super::graph_nodes;
use crate::store::node_kind;

/// A speaker once stored only where it plays, as its whole `data`; that now sits under `output`,
/// beside the volume of each wire.
pub(super) fn speakers_nest_their_output(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    for node in graph_nodes(snapshot) {
        if node_kind(node) != Some(NodeKind::Speaker.as_str()) {
            continue;
        }
        let nested = node
            .get("data")
            .and_then(Value::as_object)
            .is_some_and(current);
        if !nested {
            let output = node.get("data").cloned().unwrap_or(Value::Null);
            node["data"] = json!({ "output": output });
        }
    }
    Vec::new()
}

fn current(data: &Map<String, Value>) -> bool {
    !data.contains_key("server")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speaker(data: Option<Value>) -> Value {
        let mut node = json!({ "id": "spk", "kind": "speaker" });
        if let Some(data) = data {
            node["data"] = data;
        }
        json!({ "graph": { "nodes": [node] } })
    }

    fn nested(data: Option<Value>) -> Value {
        let mut value = speaker(data);
        assert!(speakers_nest_their_output(&mut value).is_empty());
        value["graph"]["nodes"][0]["data"].clone()
    }

    #[test]
    fn a_stored_output_moves_under_output() {
        assert_eq!(nested(Some(json!("js8"))), json!({ "output": "js8" }));
        assert_eq!(
            nested(Some(json!({ "server": "hw" }))),
            json!({ "output": { "server": "hw" } })
        );
        assert_eq!(nested(Some(Value::Null)), json!({ "output": null }));
        assert_eq!(nested(None), json!({ "output": null }));
    }

    #[test]
    fn a_current_speaker_is_left_alone() {
        let current = json!({ "output": { "server": null }, "volumes": { "ch": 0.5 } });
        assert_eq!(nested(Some(current.clone())), current);
        assert_eq!(nested(Some(json!({}))), json!({}));
    }
}

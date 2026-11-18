use sdrmm_wire::{NodeKind, WorkspaceNoticeKind};
use serde_json::{Value, json};

use super::graph_nodes;
use crate::store::node_kind;

/// The node kinds built-in processors had before every processor ran on a processor node.
const RETIRED_PROCESSOR_KINDS: &[&str] = &[
    "df",
    "beamformer",
    "passive_radar",
    "stitch",
    "spatial_spectrum",
    "correlator",
    "polarimeter",
];

/// A stored built-in processor node becomes a processor node running the same type on the same
/// settings, so its wires and id stay.
pub(super) fn processors_become_processor_nodes(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    for node in graph_nodes(snapshot) {
        let Some(kind) = node_kind(node).filter(|kind| RETIRED_PROCESSOR_KINDS.contains(kind))
        else {
            continue;
        };
        let processor_type = kind.to_owned();
        let settings = node
            .get("data")
            .and_then(|data| data.get("settings"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        node["kind"] = Value::String(NodeKind::Processor.as_str().to_owned());
        node["data"] = json!({ "processor_type": processor_type, "settings": settings });
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{NodeBody, PatchNode};

    use super::*;

    fn snapshot(node: &Value) -> Value {
        json!({ "graph": { "nodes": [node] } })
    }

    #[test]
    fn a_built_in_processor_node_keeps_its_type_and_settings() {
        let mut value = snapshot(&json!({
            "id": "finder",
            "kind": "df",
            "data": { "settings": { "algorithm": "capon", "report_ms": 250 } },
            "position": { "x": 0.0, "y": 0.0 }
        }));
        assert!(processors_become_processor_nodes(&mut value).is_empty());
        let node: PatchNode =
            serde_json::from_value(value["graph"]["nodes"][0].clone()).expect("a processor node");
        let NodeBody::Processor(processor) = node.body else {
            panic!("a processor node");
        };
        assert_eq!(processor.processor_type, "df");
        assert_eq!(
            processor.settings,
            json!({ "algorithm": "capon", "report_ms": 250 })
        );
    }

    #[test]
    fn a_node_without_settings_takes_the_defaults_and_a_rerun_changes_nothing() {
        let mut value = snapshot(&json!({ "id": "radar", "kind": "passive_radar" }));
        processors_become_processor_nodes(&mut value);
        let once = value.clone();
        processors_become_processor_nodes(&mut value);
        assert_eq!(value, once);
        assert_eq!(
            value["graph"]["nodes"][0]["data"],
            json!({ "processor_type": "passive_radar", "settings": {} })
        );
    }
}

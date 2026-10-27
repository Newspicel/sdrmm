use sdrmm_wire::{DeviceBackend, NodeKind, WorkspaceNoticeKind, generator_key};
use serde_json::{Value, json};

use super::graph_nodes;
use crate::store::node_kind;

const RETIRED_RECORDING: &str = "recording";
const RETIRED_GENERATOR: &str = "signal_gen";

/// A radio is picked by a Device node and by nothing else, so the two sources that opened one of
/// their own become Device nodes pinned to the backend they always used.
pub(super) fn sources_become_device_nodes(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    for node in graph_nodes(snapshot) {
        let Some(kind) = node_kind(node).map(str::to_owned) else {
            continue;
        };
        let Some(id) = node.get("id").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        let data = node.get("data").cloned().unwrap_or_else(|| json!({}));
        let pinned = match kind.as_str() {
            RETIRED_RECORDING => played(&data),
            RETIRED_GENERATOR => Some(generated(&id, &data)),
            _ => continue,
        };
        node["kind"] = Value::String(NodeKind::Device.as_str().to_owned());
        node["data"] = pinned.unwrap_or_else(|| json!({}));
    }
    Vec::new()
}

fn played(data: &Value) -> Option<Value> {
    let stem = data.get("recording").and_then(Value::as_str)?;
    Some(json!({
        "device": { "backend": DeviceBackend::Recording.as_str(), "key": stem }
    }))
}

fn generated(node_id: &str, data: &Value) -> Value {
    let running = data.get("running").and_then(Value::as_bool).unwrap_or(true);
    json!({
        "device": {
            "backend": DeviceBackend::Virtual.as_str(),
            "key": generator_key(node_id),
        },
        "stopped": !running,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(nodes: Vec<Value>) -> Value {
        json!({ "graph": { "nodes": nodes } })
    }

    fn stored(snapshot: &Value, index: usize) -> &Value {
        &snapshot["graph"]["nodes"][index]
    }

    #[test]
    fn a_recording_node_becomes_a_device_pinned_to_its_stem() {
        let mut value = snapshot(vec![json!({
            "id": "rec",
            "kind": RETIRED_RECORDING,
            "data": { "recording": "2026-09-16T10-00-00" },
        })]);
        assert!(sources_become_device_nodes(&mut value).is_empty());
        let node = stored(&value, 0);
        assert_eq!(node["kind"], NodeKind::Device.as_str());
        assert_eq!(node["data"]["device"]["backend"], "recording");
        assert_eq!(node["data"]["device"]["key"], "2026-09-16T10-00-00");
    }

    #[test]
    fn a_recording_node_that_named_no_file_becomes_an_empty_device() {
        let mut value = snapshot(vec![json!({
            "id": "rec",
            "kind": RETIRED_RECORDING,
            "data": {},
        })]);
        sources_become_device_nodes(&mut value);
        let node = stored(&value, 0);
        assert_eq!(node["kind"], NodeKind::Device.as_str());
        assert!(node["data"].get("device").is_none());
    }

    #[test]
    fn a_generator_node_becomes_a_device_pinned_to_its_own_key() {
        let mut value = snapshot(vec![json!({
            "id": "signal_gen:9f2c",
            "kind": RETIRED_GENERATOR,
            "data": { "running": false },
        })]);
        sources_become_device_nodes(&mut value);
        let node = stored(&value, 0);
        assert_eq!(node["kind"], NodeKind::Device.as_str());
        assert_eq!(node["data"]["device"]["backend"], "virtual");
        assert_eq!(node["data"]["device"]["key"], "gen-signal_gen-9f2c");
        assert_eq!(node["data"]["stopped"], true);
    }

    #[test]
    fn a_running_generator_comes_back_running() {
        let mut value = snapshot(vec![json!({
            "id": "gen",
            "kind": RETIRED_GENERATOR,
            "data": {},
        })]);
        sources_become_device_nodes(&mut value);
        assert_eq!(stored(&value, 0)["data"]["stopped"], false);
    }

    #[test]
    fn a_device_node_is_left_alone() {
        let mut value = snapshot(vec![json!({
            "id": "dev",
            "kind": NodeKind::Device.as_str(),
            "data": { "device": { "backend": "rtlsdr", "key": "0" } },
        })]);
        sources_become_device_nodes(&mut value);
        assert_eq!(stored(&value, 0)["data"]["device"]["backend"], "rtlsdr");
    }
}

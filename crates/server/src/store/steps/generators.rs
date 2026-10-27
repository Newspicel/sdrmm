use sdrmm_wire::{DeviceBackend, GENERATOR_PREFIX, NodeKind, WorkspaceNoticeKind, generator_key};
use serde_json::Value;

use super::graph_nodes;
use crate::store::node_kind;

const RETIRED_DRIVER_ID: &str = "siggen";
const VIRTUAL: &str = DeviceBackend::Virtual.as_str();

pub(super) fn generators_join_the_virtual_backend(
    snapshot: &mut Value,
) -> Vec<WorkspaceNoticeKind> {
    for node in graph_nodes(snapshot) {
        if node_kind(node) != Some(NodeKind::Device.as_str()) {
            continue;
        }
        let Some(node_id) = node.get("id").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        let Some(device) = node.get_mut("data").and_then(|data| data.get_mut("device")) else {
            continue;
        };
        if device.get("backend").and_then(Value::as_str) != Some(RETIRED_DRIVER_ID) {
            continue;
        }
        let key = device
            .get("key")
            .and_then(Value::as_str)
            .map_or_else(|| generator_key(&node_id), migrated_key);
        device["backend"] = Value::String(VIRTUAL.to_owned());
        device["key"] = Value::String(key);
    }
    Vec::new()
}

fn migrated_key(stored: &str) -> String {
    if stored.starts_with(GENERATOR_PREFIX) {
        return stored.to_owned();
    }
    format!("{GENERATOR_PREFIX}{stored}")
}

#[must_use]
pub(crate) fn migrated_generator_device_id(device_id: &str) -> Option<String> {
    let key = device_id
        .strip_prefix(RETIRED_DRIVER_ID)?
        .strip_prefix(':')?;
    (!key.is_empty()).then(|| format!("{VIRTUAL}:{}", migrated_key(key)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device_node(backend: &str, key: Option<&str>) -> Value {
        let device = match key {
            Some(key) => serde_json::json!({ "backend": backend, "key": key }),
            None => serde_json::json!({ "backend": backend }),
        };
        serde_json::json!({
            "graph": { "nodes": [{
                "id": "gen:1",
                "kind": NodeKind::Device.as_str(),
                "data": { "device": device },
            }] }
        })
    }

    fn stored_device(snapshot: &Value) -> &Value {
        &snapshot["graph"]["nodes"][0]["data"]["device"]
    }

    #[test]
    fn a_stored_generator_moves_to_the_virtual_backend_with_a_prefixed_key() {
        let mut snapshot = device_node(RETIRED_DRIVER_ID, Some("signal_gen-a1b2"));
        assert!(generators_join_the_virtual_backend(&mut snapshot).is_empty());
        assert_eq!(stored_device(&snapshot)["backend"], VIRTUAL);
        assert_eq!(stored_device(&snapshot)["key"], "gen-signal_gen-a1b2");
    }

    #[test]
    fn a_generator_without_a_stored_key_takes_the_one_its_node_derives() {
        let mut snapshot = device_node(RETIRED_DRIVER_ID, None);
        generators_join_the_virtual_backend(&mut snapshot);
        assert_eq!(stored_device(&snapshot)["key"], generator_key("gen:1"));
    }

    #[test]
    fn a_key_that_already_carries_the_prefix_is_left_alone() {
        let mut snapshot = device_node(RETIRED_DRIVER_ID, Some("gen-node"));
        generators_join_the_virtual_backend(&mut snapshot);
        assert_eq!(stored_device(&snapshot)["key"], "gen-node");
    }

    #[test]
    fn other_backends_are_untouched() {
        let mut snapshot = device_node("rtlsdr", Some("0"));
        generators_join_the_virtual_backend(&mut snapshot);
        assert_eq!(stored_device(&snapshot)["backend"], "rtlsdr");
        assert_eq!(stored_device(&snapshot)["key"], "0");
    }

    #[test]
    fn a_stored_device_id_is_rewritten_once() {
        assert_eq!(
            migrated_generator_device_id("siggen:signal_gen-a1b2").as_deref(),
            Some("virtual:gen-signal_gen-a1b2")
        );
        assert_eq!(
            migrated_generator_device_id("siggen:gen-node").as_deref(),
            Some("virtual:gen-node")
        );
        assert!(migrated_generator_device_id("virtual:band").is_none());
        assert!(migrated_generator_device_id("siggen:").is_none());
        assert!(migrated_generator_device_id("siggen").is_none());
    }
}

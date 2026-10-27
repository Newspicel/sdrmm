use sdrmm_wire::{DeviceBackend, NodeKind, WorkspaceNoticeKind};
use serde_json::Value;

use super::graph_nodes;
use crate::store::node_kind;

/// A backend this build no longer knows cannot be read back as a typed patch, so the pin is
/// cleared and the node waits for the operator to pick a radio again.
pub(super) fn clear_unknown_backends(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    let mut cleared = Vec::new();
    for node in graph_nodes(snapshot) {
        if node_kind(node) != Some(NodeKind::Device.as_str()) {
            continue;
        }
        let Some(id) = node.get("id").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        let Some(data) = node.get_mut("data").and_then(Value::as_object_mut) else {
            continue;
        };
        let known = data
            .get("device")
            .and_then(|device| device.get("backend"))
            .and_then(Value::as_str)
            .is_none_or(|backend| DeviceBackend::parse(backend).is_some());
        if known {
            continue;
        }
        data.remove("device");
        cleared.push(id);
    }
    if cleared.is_empty() {
        return Vec::new();
    }
    vec![WorkspaceNoticeKind::ClearedDevices { nodes: cleared }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(backend: Option<&str>) -> Value {
        let data = match backend {
            Some(backend) => serde_json::json!({ "device": { "backend": backend, "key": "0" } }),
            None => serde_json::json!({}),
        };
        serde_json::json!({
            "graph": { "nodes": [{
                "id": "dev",
                "kind": NodeKind::Device.as_str(),
                "data": data,
            }] }
        })
    }

    fn device(snapshot: &Value) -> &Value {
        &snapshot["graph"]["nodes"][0]["data"]
    }

    #[test]
    fn a_backend_this_build_dropped_leaves_the_node_without_a_radio() {
        let mut stored = snapshot(Some("kraken"));
        assert_eq!(
            clear_unknown_backends(&mut stored),
            vec![WorkspaceNoticeKind::ClearedDevices {
                nodes: vec!["dev".to_owned()]
            }]
        );
        assert!(device(&stored).get("device").is_none());
    }

    #[test]
    fn a_known_backend_is_left_alone() {
        let mut stored = snapshot(Some(DeviceBackend::Rtlsdr.as_str()));
        assert!(clear_unknown_backends(&mut stored).is_empty());
        assert_eq!(device(&stored)["device"]["backend"], "rtlsdr");
    }

    #[test]
    fn a_node_with_no_radio_yet_raises_nothing() {
        let mut stored = snapshot(None);
        assert!(clear_unknown_backends(&mut stored).is_empty());
    }
}

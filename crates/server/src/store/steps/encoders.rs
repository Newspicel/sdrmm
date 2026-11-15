use sdrmm_wire::{NodeKind, WorkspaceNoticeKind};
use serde_json::Value;

use super::graph_nodes;
use crate::store::node_kind;

const RETIRED_MODULATOR: &str = "modulator";

/// A modulator ran the transmit half of a decoder; it is now an encoder, a node of its own kind
/// that names its mode by encoder type. Settings an encoder shares keep their value, the rest
/// fall back to its defaults.
pub(super) fn modulators_become_encoders(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    for node in graph_nodes(snapshot) {
        if node_kind(node) != Some(RETIRED_MODULATOR) {
            continue;
        }
        if let Some(fields) = node.as_object_mut() {
            fields.insert("kind".to_owned(), NodeKind::Encoder.as_str().into());
        }
        let Some(data) = node.get_mut("data").and_then(Value::as_object_mut) else {
            continue;
        };
        if let Some(mode) = data.remove("channel_type") {
            data.insert("encoder_type".to_owned(), mode);
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_modulator_becomes_an_encoder_of_the_same_mode() {
        let mut snapshot = serde_json::json!({
            "graph": { "nodes": [
                {
                    "id": "mod",
                    "kind": "modulator",
                    "data": { "channel_type": "nfm", "settings": { "bandwidth_hz": 25_000 } },
                    "position": { "x": 0.0, "y": 0.0 },
                },
                { "id": "mic", "kind": "microphone", "data": {}, "position": { "x": 0.0, "y": 0.0 } },
            ] }
        });
        assert!(modulators_become_encoders(&mut snapshot).is_empty());
        assert_eq!(
            snapshot["graph"]["nodes"][0],
            serde_json::json!({
                "id": "mod",
                "kind": "encoder",
                "data": { "encoder_type": "nfm", "settings": { "bandwidth_hz": 25_000 } },
                "position": { "x": 0.0, "y": 0.0 },
            })
        );
        assert_eq!(snapshot["graph"]["nodes"][1]["kind"], "microphone");
    }
}

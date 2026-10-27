use sdrmm_wire::{NodeKind, WorkspaceNoticeKind};
use serde_json::{Value, json};

use super::graph_nodes;
use crate::store::node_kind;

const RETIRED_TIME_MACHINE: &str = "time_machine";

/// A recorder with a pre-roll is the time machine, so a stored one becomes a Recorder keeping the
/// window it was armed with.
pub(super) fn time_machines_become_recorders(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    for node in graph_nodes(snapshot) {
        if node_kind(node) != Some(RETIRED_TIME_MACHINE) {
            continue;
        }
        let data = node.get("data").cloned().unwrap_or_else(|| json!({}));
        let armed = data.get("armed").and_then(Value::as_bool).unwrap_or(false);
        let seconds = data
            .get("history_seconds")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let pre_roll_s = if armed { seconds } else { 0 };
        node["kind"] = Value::String(NodeKind::Recorder.as_str().to_owned());
        node["data"] = json!({ "recording": false, "pre_roll_s": pre_roll_s });
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(data: Value) -> Value {
        json!({
            "graph": { "nodes": [{ "id": "tm", "kind": RETIRED_TIME_MACHINE, "data": data }] }
        })
    }

    fn stored(snapshot: &Value) -> &Value {
        &snapshot["graph"]["nodes"][0]
    }

    #[test]
    fn an_armed_time_machine_keeps_its_window_as_a_pre_roll() {
        let mut value = snapshot(json!({ "history_seconds": 30, "armed": true }));
        assert!(time_machines_become_recorders(&mut value).is_empty());
        assert_eq!(stored(&value)["kind"], NodeKind::Recorder.as_str());
        assert_eq!(stored(&value)["data"]["pre_roll_s"], 30);
        assert_eq!(stored(&value)["data"]["recording"], false);
    }

    #[test]
    fn a_time_machine_that_was_not_holding_becomes_a_plain_recorder() {
        let mut value = snapshot(json!({ "history_seconds": 30, "armed": false }));
        time_machines_become_recorders(&mut value);
        assert_eq!(stored(&value)["data"]["pre_roll_s"], 0);
    }

    #[test]
    fn a_recorder_is_left_alone() {
        let mut value = json!({
            "graph": { "nodes": [{
                "id": "rec",
                "kind": NodeKind::Recorder.as_str(),
                "data": { "recording": true },
            }] }
        });
        time_machines_become_recorders(&mut value);
        assert_eq!(value["graph"]["nodes"][0]["data"]["recording"], true);
        assert!(
            value["graph"]["nodes"][0]["data"]
                .get("pre_roll_s")
                .is_none()
        );
    }
}

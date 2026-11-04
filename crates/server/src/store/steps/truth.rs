use sdrmm_wire::{NodeKind, RADAR_TRUTH_PORT, WorkspaceNoticeKind};
use serde_json::Value;

use super::{graph_nodes, wires};
use crate::store::node_kind;

/// The port a passive radar took its truth on before any tracked kind could feed it.
const OLD_TRUTH_PORT: &str = "adsb";

pub(super) fn truth_from_adsb(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    let radars: Vec<String> = graph_nodes(snapshot)
        .filter(|node| node_kind(node) == Some(NodeKind::PassiveRadar.as_str()))
        .filter_map(|node| Some(node.get("id")?.as_str()?.to_owned()))
        .collect();
    for edge in wires::graph_edges(snapshot).into_iter().flatten() {
        let Some(to) = wires::end(edge, "to") else {
            continue;
        };
        if to.port == OLD_TRUTH_PORT && radars.contains(&to.node) {
            edge["to"]["port"] = Value::from(RADAR_TRUTH_PORT);
        }
    }
    Vec::new()
}

use sdrmm_wire::WorkspaceNoticeKind;
use serde_json::Value;

use super::{graph_nodes, wires};
use crate::store::node_kind;

/// The node kind a passive radar had before every processor ran on a processor node.
const OLD_RADAR_KIND: &str = "passive_radar";
/// The port a passive radar took its truth on before any tracked kind could feed it.
const OLD_TRUTH_PORT: &str = "adsb";
const TRUTH_PORT: &str = "truth";

pub(super) fn truth_from_adsb(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    let radars: Vec<String> = graph_nodes(snapshot)
        .filter(|node| node_kind(node) == Some(OLD_RADAR_KIND))
        .filter_map(|node| Some(node.get("id")?.as_str()?.to_owned()))
        .collect();
    for edge in wires::graph_edges(snapshot).into_iter().flatten() {
        let Some(to) = wires::end(edge, "to") else {
            continue;
        };
        if to.port == OLD_TRUTH_PORT && radars.contains(&to.node) {
            edge["to"]["port"] = Value::from(TRUTH_PORT);
        }
    }
    Vec::new()
}

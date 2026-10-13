use sdrmm_wire::{NodeKind, WorkspaceNoticeKind, port_stream};
use serde_json::Value;

use super::{graph_nodes, wires};
use crate::store::node_kind;

const TX_PORT: &str = "tx";
const NO_TRANSMIT: &str = "Radios take no transmit wire";

pub(super) fn drop_tx_wires(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    let radios: Vec<String> = graph_nodes(snapshot)
        .filter(|node| node_kind(node) == Some(NodeKind::Device.as_str()))
        .filter_map(|node| Some(node.get("id")?.as_str()?.to_owned()))
        .collect();
    wires::drop_where(snapshot, |edge| {
        let to = wires::end(edge, "to")?;
        (radios.contains(&to.node) && port_stream(TX_PORT, &to.port).is_some())
            .then(|| NO_TRANSMIT.to_owned())
    })
}

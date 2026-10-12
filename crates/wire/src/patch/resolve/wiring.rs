use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{
    super::{PatchEdge, PatchGraph, PortDirection, PortRef, Refusal},
    Net, ResolveCtx, checks,
};
use crate::device::Coherence;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct WiringRequest {
    pub graph: PatchGraph,
    pub from: PortRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WireVerdict {
    pub to: PortRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<String>,
}

#[must_use]
pub fn wiring(graph: &PatchGraph, from: &PortRef, ctx: &ResolveCtx<'_>) -> Vec<WireVerdict> {
    let candidates = targets(&Net::new(graph, ctx), from);
    let mut net = Net::new(graph, ctx);
    checks::wire(&mut net);
    candidates
        .iter()
        .map(|edge| WireVerdict {
            to: edge.to.clone(),
            refusal: checks::refusal(&mut net, edge)
                .map(|error| error.to_string())
                .or_else(|| shared_clock(&net, edge).map(|refusal| refusal.text())),
        })
        .collect()
}

fn targets(net: &Net<'_>, from: &PortRef) -> Vec<PatchEdge> {
    let Some(out) = net.spec(from, PortDirection::Out) else {
        return Vec::new();
    };
    net.graph
        .nodes
        .iter()
        .zip(&net.ports)
        .filter(|(node, _)| node.id != from.node)
        .flat_map(|(node, ports)| {
            ports
                .iter()
                .filter(|port| port.direction == PortDirection::In)
                .filter(|port| port.port_type == out.port_type)
                .map(|port| PatchEdge {
                    from: from.clone(),
                    to: PortRef {
                        node: node.id.clone(),
                        port: port.name.clone(),
                    },
                })
        })
        .collect()
}

fn shared_clock(net: &Net<'_>, edge: &PatchEdge) -> Option<Refusal> {
    let input = net.spec(&edge.to, PortDirection::In)?;
    if !input.coherent || coherence(net, &edge.from.node)? != Coherence::None {
        return None;
    }
    net.edges
        .iter()
        .any(|kept| {
            kept.to.node == edge.to.node
                && kept.from.node == edge.from.node
                && net
                    .spec(&kept.to, PortDirection::In)
                    .is_some_and(|other| other.coherent)
        })
        .then_some(Refusal::SharedClock)
}

fn coherence(net: &Net<'_>, node: &str) -> Option<Coherence> {
    let index = net.graph.nodes.iter().position(|found| found.id == node)?;
    net.coherence.get(index).copied().flatten()
}

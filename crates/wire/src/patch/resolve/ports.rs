use super::super::{
    NodeBody, PatchGraph, PatchNode, PortDirection, PortRef, PortRepeat, PortSpec, port_index,
    stream_port,
};
use crate::{
    channel::ChannelDescriptor,
    device::{Capabilities, RadioUse},
};

/// A port the radio's use leaves out is not offered, unless a wire still holds it: that wire
/// stays drawn and carries nothing, so it never hides.
pub(super) fn concrete(
    graph: &PatchGraph,
    node: &PatchNode,
    descriptors: Option<&[ChannelDescriptor]>,
    caps: Option<&Capabilities>,
) -> Vec<PortSpec> {
    let used = radio_use(node, caps);
    let offered = |spec: &PortSpec| used.is_none_or(|used| spec.offered_in(used));
    let mut ports = Vec::new();
    for spec in node.body.ports(descriptors) {
        if spec.repeat == PortRepeat::Once {
            if offered(&spec) || holds_wire(graph, &node.id, &spec.name) {
                ports.push(spec);
            }
            continue;
        }
        let highest = highest_wired(graph, &node.id, &spec);
        for stream in 0..count(spec.repeat, caps, highest) {
            let name = stream_port(&spec.name, stream);
            if offered(&spec) || holds_wire(graph, &node.id, &name) {
                ports.push(PortSpec {
                    name,
                    repeat: PortRepeat::Once,
                    stream: Some(stream),
                    ..spec.clone()
                });
            }
        }
    }
    ports
}

fn radio_use(node: &PatchNode, caps: Option<&Capabilities>) -> Option<RadioUse> {
    match &node.body {
        NodeBody::Device(device) => Some(device.use_on(caps.map(|caps| caps.duplex))),
        _ => None,
    }
}

fn holds_wire(graph: &PatchGraph, node: &str, port: &str) -> bool {
    graph
        .edges
        .iter()
        .flat_map(|edge| [&edge.from, &edge.to])
        .any(|end| end.node == node && end.port == port)
}

pub(super) fn find<'a>(
    ports: &'a [PortSpec],
    reference: &PortRef,
    direction: PortDirection,
) -> Result<&'a PortSpec, bool> {
    ports
        .iter()
        .find(|port| port.direction == direction && port.name == reference.port)
        .ok_or_else(|| ports.iter().any(|port| port.name == reference.port))
}

fn count(repeat: PortRepeat, caps: Option<&Capabilities>, highest: Option<u32>) -> u32 {
    let wired = highest.map_or(0, |index| index.saturating_add(1));
    match repeat {
        PortRepeat::Once => 1,
        PortRepeat::PerRxStream => caps
            .map_or(1, |caps| caps.rx_streams.clamp(1, repeat.limit()))
            .max(wired)
            .min(repeat.limit()),
        PortRepeat::PerLane => wired.saturating_add(1).min(repeat.limit()),
    }
}

fn highest_wired(graph: &PatchGraph, node: &str, spec: &PortSpec) -> Option<u32> {
    graph
        .edges
        .iter()
        .flat_map(|edge| [&edge.from, &edge.to])
        .filter(|end| end.node == node)
        .filter_map(|end| port_index(&spec.name, &end.port, spec.repeat.limit()))
        .max()
}

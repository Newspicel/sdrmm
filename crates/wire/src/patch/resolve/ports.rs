use super::super::{
    PatchGraph, PatchNode, PortDirection, PortRef, PortRepeat, PortSpec, port_index, stream_port,
};
use crate::{channel::ChannelDescriptor, device::Capabilities};

pub(super) fn concrete(
    graph: &PatchGraph,
    node: &PatchNode,
    descriptors: Option<&[ChannelDescriptor]>,
    caps: Option<&Capabilities>,
) -> Vec<PortSpec> {
    let mut ports = Vec::new();
    for spec in node.body.ports(descriptors) {
        if spec.repeat == PortRepeat::Once {
            ports.push(spec);
            continue;
        }
        let highest = highest_wired(graph, &node.id, &spec);
        for stream in 0..count(spec.repeat, caps, highest) {
            ports.push(PortSpec {
                name: stream_port(&spec.name, stream),
                repeat: PortRepeat::Once,
                stream: Some(stream),
                ..spec.clone()
            });
        }
    }
    ports
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

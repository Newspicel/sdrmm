use super::{
    super::{ARRAY_LANE_PORT, PortDirection, PortType, Refusal, stream_port},
    ArrayOffer, LaneWire, ResolvedNode, ResolvedPatch, Runs,
};
use crate::{device::Coherence, state::StateSnapshot};

pub(super) fn array_offers(patch: &mut ResolvedPatch, live: &StateSnapshot) {
    let offers: Vec<Option<ArrayOffer>> = patch
        .nodes
        .iter()
        .map(|node| offer(patch, node, live))
        .collect();
    for (node, offer) in patch.nodes.iter_mut().zip(offers) {
        node.array_offer = offer;
    }
}

fn offer(patch: &ResolvedPatch, node: &ResolvedNode, live: &StateSnapshot) -> Option<ArrayOffer> {
    let Runs::Radio { device_set } = node.runs else {
        return None;
    };
    let set = live.device_sets.iter().find(|set| set.id == device_set)?;
    let lanes: Vec<&str> = (0..set.capabilities.rx_streams)
        .map(|stream| lane_output(node, stream))
        .collect::<Option<_>>()?;
    if lanes.len() < 2 || holds_a_lane(patch, node) {
        return None;
    }
    if set.capabilities.coherence == Coherence::None {
        return Some(ArrayOffer::Refused {
            reason: Refusal::SharedClock.text(),
        });
    }
    let wires = (0..)
        .zip(lanes)
        .map(|(stream, from)| LaneWire {
            from: from.to_owned(),
            to: stream_port(ARRAY_LANE_PORT, stream),
        })
        .collect();
    Some(ArrayOffer::Ready { wires })
}

fn lane_output(node: &ResolvedNode, stream: u32) -> Option<&str> {
    node.outputs()
        .find(|port| port.spec.port_type == PortType::Iq && port.spec.stream == Some(stream))
        .map(|port| port.spec.name.as_str())
}

fn holds_a_lane(patch: &ResolvedPatch, node: &ResolvedNode) -> bool {
    node.outputs()
        .filter(|port| port.spec.port_type == PortType::Iq)
        .flat_map(|port| &port.reaches)
        .any(|reach| {
            patch
                .node(&reach.sink)
                .and_then(|sink| {
                    sink.ports.iter().find(|port| {
                        port.spec.direction == PortDirection::In && port.spec.name == reach.port
                    })
                })
                .is_some_and(|port| port.spec.coherent)
        })
}

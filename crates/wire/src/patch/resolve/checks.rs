use super::{
    super::{
        IqOrigin, MAX_EDGES, MAX_NODE_ID_LEN, MAX_NODES, MAX_SIGNAL_MAP_BANDWIDTH_HZ,
        MAX_SIGNAL_MAP_OFFSET_HZ, Multi, NodeBody, PatchEdge, PatchError, PatchGraph, PatchNode,
        Payload, PortDirection, PortRef, PortSpec, Refusal, check_geometry, valid_host_port,
        validate_gps_source,
    },
    Net, feeds, ports,
};
use crate::{
    channel::ChannelDescriptor, network::MAX_NETWORK_ADDRESS_LEN, workspace::MAX_NAME_LEN,
};

pub(super) fn structure(
    graph: &PatchGraph,
    channels: Option<&[ChannelDescriptor]>,
) -> Result<(), PatchError> {
    if graph.nodes.len() > MAX_NODES {
        return Err(PatchError::TooManyNodes);
    }
    if graph.edges.len() > MAX_EDGES {
        return Err(PatchError::TooManyEdges);
    }
    let mut seen: Vec<&str> = Vec::with_capacity(graph.nodes.len());
    for node in &graph.nodes {
        if node.id.is_empty() || node.id.len() > MAX_NODE_ID_LEN {
            return Err(PatchError::NodeId(node.id.clone()));
        }
        if seen.contains(&node.id.as_str()) {
            return Err(PatchError::DuplicateNode(node.id.clone()));
        }
        seen.push(&node.id);
        if node
            .label
            .as_ref()
            .is_some_and(|label| label.is_empty() || label.chars().count() > MAX_NAME_LEN)
        {
            return Err(PatchError::Label(node.id.clone()));
        }
        check_geometry(node)?;
        settings(node, channels)?;
    }
    Ok(())
}

fn settings(node: &PatchNode, channels: Option<&[ChannelDescriptor]>) -> Result<(), PatchError> {
    let refused = || Err(PatchError::NodeSettings(node.id.clone()));
    match &node.body {
        NodeBody::Device(device) => {
            if device
                .device
                .as_ref()
                .is_some_and(|r| r.backend.is_empty() || r.backend.len() > MAX_NAME_LEN)
            {
                return Err(PatchError::Backend(node.id.clone()));
            }
        }
        NodeBody::Channel(channel) => {
            if let Some(descriptors) = channels
                && !descriptors
                    .iter()
                    .any(|d| d.type_id == channel.channel_type)
            {
                return Err(PatchError::ChannelType(channel.channel_type.clone()));
            }
        }
        NodeBody::Gps(gps) => {
            if let Some(source) = &gps.source {
                validate_gps_source(source)?;
            }
        }
        NodeBody::SignalMap(settings) => {
            if settings.offset_hz.unsigned_abs() > MAX_SIGNAL_MAP_OFFSET_HZ as u64
                || !(1..=MAX_SIGNAL_MAP_BANDWIDTH_HZ).contains(&settings.bandwidth_hz)
            {
                return refused();
            }
        }
        NodeBody::NetworkExport(export) => {
            if !export.settings.valid_format()
                || export.settings.address.is_empty()
                || export.settings.address.len() > MAX_NETWORK_ADDRESS_LEN
                || !valid_host_port(&export.settings.address)
            {
                return refused();
            }
        }
        body if !settings_valid(body) => return refused(),
        _ => {}
    }
    Ok(())
}

fn settings_valid(body: &NodeBody) -> bool {
    match body {
        NodeBody::Recording(recording) => recording.valid(),
        NodeBody::EventFilter(settings) => settings.valid(),
        NodeBody::Note(note) => note.valid(),
        NodeBody::AudioFx(fx) => fx.settings.validate().is_ok(),
        NodeBody::SpectrumMonitor(settings) => settings.valid(),
        NodeBody::Propagation(settings) => settings.valid(),
        NodeBody::TimeMachine(settings) => settings.valid(),
        NodeBody::DmrTrunk(settings) => settings.valid(),
        NodeBody::EventOutput(settings) => settings.target.valid(),
        NodeBody::Satellite(satellite) => satellite.valid(),
        NodeBody::Rotator(rotator) => rotator.valid(),
        NodeBody::Array(array) => array.valid(),
        NodeBody::Hunt(hunt) => hunt.sweep.problem().is_none(),
        NodeBody::Triangulation(triangulation) => triangulation.settings.problem().is_none(),
        body => body.processor_params().is_none_or(|params| params.valid()),
    }
}

pub(super) fn wire<'a>(net: &mut Net<'a>) -> Vec<(PatchEdge, PatchError)> {
    let graph = net.graph;
    let mut refused = Vec::new();
    for (index, edge) in graph.edges.iter().enumerate() {
        if index >= MAX_EDGES {
            refused.push((edge.clone(), PatchError::TooManyEdges));
            continue;
        }
        match refusal(net, edge) {
            None => net.edges.push(edge),
            Some(error) => refused.push((edge.clone(), error)),
        }
    }
    refused
}

pub(super) fn refusal<'a>(net: &mut Net<'a>, edge: &'a PatchEdge) -> Option<PatchError> {
    if edge.from.node == edge.to.node {
        return Some(PatchError::SelfEdge(edge.from.node.clone()));
    }
    let out = match endpoint(net, &edge.from, PortDirection::Out) {
        Ok(spec) => spec,
        Err(error) => return Some(error),
    };
    let input = match endpoint(net, &edge.to, PortDirection::In) {
        Ok(spec) => spec,
        Err(error) => return Some(error),
    };
    if out.port_type != input.port_type {
        return Some(PatchError::TypeMismatch {
            from: out.port_type,
            to: input.port_type,
        });
    }
    if net
        .edges
        .iter()
        .any(|kept| kept.from == edge.from && kept.to == edge.to)
    {
        return Some(PatchError::DuplicateEdge(edge.to.clone()));
    }
    let refused = |refusal: Refusal| {
        Some(PatchError::Refused {
            edge: Box::new(edge.clone()),
            refusal,
        })
    };
    if let Some(found) = arity(net, edge, &out, &input) {
        return refused(found);
    }
    if reaches(net, &edge.to.node, &edge.from.node) {
        return Some(PatchError::Cycle(edge.to.node.clone()));
    }
    net.edges.push(edge);
    let found = admission(net, edge, &input);
    net.edges.pop();
    found.and_then(refused)
}

fn endpoint(
    net: &Net<'_>,
    reference: &PortRef,
    direction: PortDirection,
) -> Result<PortSpec, PatchError> {
    let Some((_, specs)) = net.node(&reference.node) else {
        return Err(PatchError::UnknownNode(reference.node.clone()));
    };
    match ports::find(specs, reference, direction) {
        Ok(spec) => Ok(spec.clone()),
        Err(true) => Err(PatchError::Direction(reference.clone())),
        Err(false) => Err(PatchError::UnknownPort(reference.clone())),
    }
}

fn arity(net: &Net<'_>, edge: &PatchEdge, out: &PortSpec, input: &PortSpec) -> Option<Refusal> {
    let landed = net.edges.iter().filter(|kept| kept.to.node == edge.to.node);
    if input.multi == Multi::One && landed.clone().any(|kept| kept.to.port == edge.to.port) {
        return Some(Refusal::OneWire);
    }
    if out.multi == Multi::One && net.edges.iter().any(|kept| kept.from == edge.from) {
        return Some(Refusal::OneTarget);
    }
    if let Some(group) = &input.group
        && landed.clone().any(|kept| {
            kept.to.port != edge.to.port
                && net
                    .spec(&kept.to, PortDirection::In)
                    .is_some_and(|other| other.group.as_ref() == Some(group))
        })
    {
        return Some(Refusal::Group);
    }
    if input.exclusive
        && let Some(holder) = net.edges.iter().find(|kept| {
            kept.from == edge.from
                && net
                    .spec(&kept.to, PortDirection::In)
                    .is_some_and(|other| other.exclusive)
        })
    {
        return Some(Refusal::LaneTaken {
            holder: net.label(&holder.to.node),
        });
    }
    None
}

fn reaches(net: &Net<'_>, from: &str, goal: &str) -> bool {
    let mut seen: Vec<&str> = Vec::new();
    let mut open = vec![from];
    while let Some(node) = open.pop() {
        if node == goal {
            return true;
        }
        if seen.contains(&node) {
            continue;
        }
        seen.push(node);
        open.extend(
            net.edges
                .iter()
                .filter(|edge| edge.from.node == node)
                .map(|edge| edge.to.node.as_str()),
        );
    }
    false
}

fn admission(net: &Net<'_>, edge: &PatchEdge, input: &PortSpec) -> Option<Refusal> {
    feeds::downstream(net, &edge.to, input.port_type)
        .iter()
        .find_map(|reference| {
            let spec = net.spec(reference, PortDirection::In)?;
            let fed = feeds::feeds_into(net, &reference.node, spec);
            judge(net, &reference.node, spec, &fed)
        })
}

fn judge(net: &Net<'_>, node: &str, spec: &PortSpec, fed: &[super::Feed]) -> Option<Refusal> {
    if spec.direct && fed.iter().any(|feed| !feed.via.is_empty()) {
        return Some(Refusal::Direct);
    }
    if spec.multi == Multi::ManyLanes
        && fed.len() > 1
        && !fed.iter().all(|feed| lane(&feed.payload))
    {
        return Some(Refusal::LanesOnly);
    }
    fed.iter()
        .any(|feed| !spec.payload.admits(&feed.payload))
        .then(|| Refusal::Takes {
            node: net.label(node),
            what: spec
                .takes
                .clone()
                .unwrap_or_else(|| spec.port_type.name().to_lowercase()),
        })
}

fn lane(payload: &Payload) -> bool {
    matches!(payload, Payload::Iq { origins } if origins
        .iter()
        .all(|origin| matches!(origin, IqOrigin::Radio | IqOrigin::Synthetic)))
}

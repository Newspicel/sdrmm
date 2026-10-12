use super::{
    super::{NodeBody, PortDirection, PortRef, PortSpec, PortType, Role},
    Feed, Net, Reach, ResolvedPort,
};
use crate::{
    decode::KindSet,
    patch::{Keeps, Payload},
};

pub const MAX_PASS_DEPTH: usize = 16;

pub(super) fn resolve_port(net: &Net<'_>, node: &str, spec: PortSpec) -> ResolvedPort {
    match spec.direction {
        PortDirection::In => {
            let feeds = feeds_into(net, node, &spec);
            let offered = feeds
                .iter()
                .fold(KindSet::EMPTY, |all, feed| all.union(feed.payload.kinds()));
            ResolvedPort {
                ignored: offered.without(spec.payload.kinds()),
                spec,
                feeds,
                reaches: Vec::new(),
                kept: Vec::new(),
            }
        }
        PortDirection::Out => {
            let spec = passed_on(net, node, spec);
            let reaches = reaches_from(net, node, &spec);
            ResolvedPort {
                spec,
                feeds: Vec::new(),
                ignored: KindSet::EMPTY,
                kept: kept_by(net, &reaches),
                reaches,
            }
        }
    }
}

fn kept_by(net: &Net<'_>, reaches: &[Reach]) -> Vec<Keeps> {
    let mut kept = Vec::new();
    for keeps in reaches.iter().filter_map(|reach| {
        net.spec(&reference(&reach.sink, &reach.port), PortDirection::In)?
            .keeps
    }) {
        if !kept.contains(&keeps) {
            kept.push(keeps);
        }
    }
    kept
}

pub(super) fn feeds_into(net: &Net<'_>, node: &str, spec: &PortSpec) -> Vec<Feed> {
    let mut feeds = Vec::new();
    back(
        net,
        (node, &spec.name),
        spec.port_type,
        &mut Vec::new(),
        &mut feeds,
    );
    feeds
}

pub(super) fn downstream(net: &Net<'_>, to: &PortRef, port_type: PortType) -> Vec<PortRef> {
    if !passes(net, &to.node, port_type) {
        return vec![to.clone()];
    }
    let mut reaches = Vec::new();
    let mut via = vec![to.node.clone()];
    for output in pass_ports(net, &to.node, port_type, PortDirection::Out) {
        forward(
            net,
            (&to.node, &output),
            port_type,
            KindSet::EMPTY,
            &mut via,
            &mut reaches,
        );
    }
    let mut inputs = vec![to.clone()];
    inputs.extend(reaches.into_iter().map(|reach| PortRef {
        node: reach.sink,
        port: reach.port,
    }));
    inputs
}

fn passes(net: &Net<'_>, node: &str, port_type: PortType) -> bool {
    net.node(node)
        .is_some_and(|(found, _)| found.body.role() == Role::Pass(port_type))
}

fn pass_ports(
    net: &Net<'_>,
    node: &str,
    port_type: PortType,
    direction: PortDirection,
) -> Vec<String> {
    net.ports_of(node)
        .iter()
        .filter(|port| port.direction == direction && port.port_type == port_type)
        .map(|port| port.name.clone())
        .collect()
}

fn through(net: &Net<'_>, pass: &str, payload: Payload) -> Payload {
    match (net.node(pass).map(|(node, _)| &node.body), payload) {
        (Some(NodeBody::EventFilter(filter)), Payload::Events { kinds }) => Payload::Events {
            kinds: filter.narrow(kinds),
        },
        (_, payload) => payload,
    }
}

fn passed_on(net: &Net<'_>, node: &str, spec: PortSpec) -> PortSpec {
    if !passes(net, node, spec.port_type) || spec.port_type != PortType::Events {
        return spec;
    }
    let offered = pass_ports(net, node, spec.port_type, PortDirection::In)
        .iter()
        .filter_map(|name| net.spec(&reference(node, name), PortDirection::In))
        .flat_map(|input| feeds_into(net, node, input))
        .fold(KindSet::EMPTY, |all, feed| all.union(feed.payload.kinds()));
    let payload = through(net, node, Payload::Events { kinds: offered });
    PortSpec { payload, ..spec }
}

fn reference(node: &str, port: &str) -> PortRef {
    PortRef {
        node: node.to_owned(),
        port: port.to_owned(),
    }
}

fn back(
    net: &Net<'_>,
    (node, port): (&str, &str),
    port_type: PortType,
    via: &mut Vec<String>,
    feeds: &mut Vec<Feed>,
) {
    for edge in net
        .edges
        .iter()
        .filter(|edge| edge.to.node == node && edge.to.port == port)
    {
        let source = edge.from.node.as_str();
        if !passes(net, source, port_type) {
            let Some(out) = net.spec(&edge.from, PortDirection::Out) else {
                continue;
            };
            let via: Vec<String> = via.iter().rev().cloned().collect();
            let payload = via.iter().fold(out.payload.clone(), |payload, pass| {
                through(net, pass, payload)
            });
            feeds.push(Feed {
                source: source.to_owned(),
                port: edge.from.port.clone(),
                via,
                payload,
                tap: None,
            });
            continue;
        }
        if via.len() >= MAX_PASS_DEPTH || via.iter().any(|seen| seen == source) {
            continue;
        }
        via.push(source.to_owned());
        for input in pass_ports(net, source, port_type, PortDirection::In) {
            back(net, (source, &input), port_type, via, feeds);
        }
        via.pop();
    }
}

fn reaches_from(net: &Net<'_>, node: &str, spec: &PortSpec) -> Vec<Reach> {
    let mut reaches = Vec::new();
    forward(
        net,
        (node, &spec.name),
        spec.port_type,
        spec.payload.kinds(),
        &mut Vec::new(),
        &mut reaches,
    );
    reaches
}

fn forward(
    net: &Net<'_>,
    (node, port): (&str, &str),
    port_type: PortType,
    kinds: KindSet,
    via: &mut Vec<String>,
    reaches: &mut Vec<Reach>,
) {
    for edge in net
        .edges
        .iter()
        .filter(|edge| edge.from.node == node && edge.from.port == port)
    {
        let target = edge.to.node.as_str();
        if !passes(net, target, port_type) {
            let accepted = net
                .spec(&edge.to, PortDirection::In)
                .map_or(KindSet::EMPTY, |input| input.payload.kinds());
            reaches.push(Reach {
                sink: target.to_owned(),
                port: edge.to.port.clone(),
                via: via.clone(),
                kinds: kinds.intersection(accepted),
            });
            continue;
        }
        if via.len() >= MAX_PASS_DEPTH || via.iter().any(|seen| seen == target) {
            continue;
        }
        let narrowed = through(net, target, Payload::Events { kinds }).kinds();
        via.push(target.to_owned());
        for output in pass_ports(net, target, port_type, PortDirection::Out) {
            forward(net, (target, &output), port_type, narrowed, via, reaches);
        }
        via.pop();
    }
}

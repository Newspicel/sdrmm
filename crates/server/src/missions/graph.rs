use sdrmm_wire::{
    ControlVerb, Feed, GpsNode, MissionProblem, NodeBody, POSITION_PORT, PatchGraph, PortType,
    PositionLink, PositionSource,
};

use crate::{control, phones::Phones, resolved::Resolved};

pub(crate) fn source_of<'a>(resolved: &'a Resolved, node: &str, port: &str) -> Option<&'a Feed> {
    resolved.patch.first_feed(node, port)
}

pub(crate) fn event_sources(resolved: &Resolved, node: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let Some(sink) = resolved.patch.node(node) else {
        return found;
    };
    for feed in sink
        .inputs()
        .filter(|port| port.spec.port_type == PortType::Events)
        .flat_map(|port| &port.feeds)
    {
        if !found.contains(&feed.source) {
            found.push(feed.source.clone());
        }
    }
    found
}

pub(crate) fn position_link(
    resolved: &Resolved,
    phones: &Phones,
    node: &str,
) -> (Option<PositionLink>, Option<MissionProblem>) {
    link(resolved, phones, node, POSITION_PORT)
}

pub(crate) fn link(
    resolved: &Resolved,
    phones: &Phones,
    node: &str,
    port: &str,
) -> (Option<PositionLink>, Option<MissionProblem>) {
    let Some(source) = source_of(resolved, node, port) else {
        return (None, None);
    };
    let phone = phone_of(&resolved.graph, &source.source);
    let problem = phone
        .as_deref()
        .and_then(|phone| phone_problem(phones, phone));
    (
        Some(PositionLink {
            node: source.source.clone(),
            phone,
        }),
        problem,
    )
}

pub(crate) fn controlled_channel(resolved: &Resolved, hunt: &str) -> Option<String> {
    control::targets(&resolved.patch, hunt)
        .into_iter()
        .find(|(_, verbs)| verbs.contains(&ControlVerb::Sweep))
        .map(|(target, _)| target)
}

fn phone_of(graph: &PatchGraph, gps: &str) -> Option<String> {
    match &graph.node(gps)?.body {
        NodeBody::Gps(GpsNode {
            source: Some(PositionSource::Phone { phone }),
        }) => Some(phone.clone()),
        _ => None,
    }
}

fn phone_problem(phones: &Phones, phone: &str) -> Option<MissionProblem> {
    if !phones.known(phone) {
        return Some(MissionProblem::PhoneNotPaired {
            phone: phone.to_owned(),
        });
    }
    (!phones.online(phone)).then(|| MissionProblem::PhoneOffline {
        phone: phone.to_owned(),
    })
}

use super::{
    super::{ARRAY_PORT, IQ_PORT, NodeBody, PatchGraph, PortDirection, PortType, Refusal},
    ChannelAddr, Feed, NodeProblem, ProblemSource, ResolvedNode, ResolvedPatch, Runs, Tap,
    UnboundReason,
};
use crate::{
    control::ControlVerb,
    device::{Coherence, DeviceInfo},
    state::{DeviceSet, StateSnapshot},
};

#[must_use]
pub fn bound_devices(graph: &PatchGraph, live: &StateSnapshot) -> Vec<(String, u32)> {
    let mut bound = Vec::new();
    let mut claimed: Vec<u32> = Vec::new();
    for node in graph.device_nodes() {
        let Some(reference) = node.body.device_ref(&node.id) else {
            continue;
        };
        if let Some(set) = live
            .device_sets
            .iter()
            .find(|set| !claimed.contains(&set.id) && reference.matches(&set.device))
        {
            claimed.push(set.id);
            bound.push((node.id.clone(), set.id));
        }
    }
    bound
}

pub fn bind(
    patch: &mut ResolvedPatch,
    graph: &PatchGraph,
    live: &StateSnapshot,
    attached: &[DeviceInfo],
) -> Vec<NodeProblem> {
    let devices = bound_devices(graph, live);
    let mut problems = Vec::new();
    for node in &mut patch.nodes {
        if let Some(runs) = radio_runs(graph, &node.id, (&devices, attached), &mut problems) {
            node.runs = runs;
        }
    }
    for node in &mut patch.nodes {
        tap_lanes(node, graph, &devices, live);
    }
    let radios = runs_of(patch);
    for node in &mut patch.nodes {
        if let Some(runs) = channel_runs(graph, node, (live, &radios), &mut problems) {
            node.runs = runs;
        }
    }
    let runs = runs_of(patch);
    for node in &mut patch.nodes {
        tap_channels(node, &runs);
        reach_targets(node, &runs);
        drive_feeds(node, live);
    }
    let following: Vec<String> = patch
        .nodes
        .iter()
        .filter(|node| lane_follows(patch, graph, &node.id))
        .map(|node| node.id.clone())
        .collect();
    for edge in &mut patch.edges {
        edge.retunes_source = edge.to.port == IQ_PORT && following.contains(&edge.to.node);
    }
    super::offer::array_offers(patch, live);
    problems.extend(
        patch
            .nodes
            .iter()
            .filter(|node| shares_no_clock(node, &devices, live))
            .map(|node| problem(&node.id, &Refusal::SharedClock.text())),
    );
    problems
}

fn runs_of(patch: &ResolvedPatch) -> Vec<(String, Runs)> {
    patch
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node.runs.clone()))
        .collect()
}

fn runs_at<'a>(runs: &'a [(String, Runs)], node: &str) -> Option<&'a Runs> {
    runs.iter().find(|(id, _)| id == node).map(|(_, runs)| runs)
}

fn reach_targets(node: &mut ResolvedNode, runs: &[(String, Runs)]) {
    for port in node
        .ports
        .iter_mut()
        .filter(|port| port.spec.port_type == PortType::Control)
        .filter(|port| port.spec.direction == PortDirection::Out)
    {
        for reach in &mut port.reaches {
            reach.target_runs = runs_at(runs, &reach.sink).cloned();
        }
    }
}

fn drive_feeds(node: &mut ResolvedNode, live: &StateSnapshot) {
    let Runs::Channel {
        device_set,
        channel,
        ..
    } = node.runs
    else {
        return;
    };
    let Some(set) = live.device_sets.iter().find(|set| set.id == device_set) else {
        return;
    };
    for port in node
        .ports
        .iter_mut()
        .filter(|port| port.spec.port_type == PortType::Control)
        .filter(|port| port.spec.direction == PortDirection::In)
    {
        for feed in &mut port.feeds {
            feed.drives = sweeps(set, channel, &feed.source).then_some(ControlVerb::Sweep);
        }
    }
}

fn sweeps(set: &DeviceSet, channel: u32, from: &str) -> bool {
    let by = |node: Option<&String>| node.is_some_and(|node| node == from);
    set.scanners.iter().any(|scanner| {
        scanner.error.is_none()
            && scanner.settings.channel == channel
            && by(scanner.settings.node.as_ref())
    }) || set
        .hunts
        .iter()
        .any(|hunt| hunt.settings.channel == channel && by(hunt.settings.node.as_ref()))
}

fn shares_no_clock(node: &ResolvedNode, devices: &[(String, u32)], live: &StateSnapshot) -> bool {
    let sources: Vec<&str> = node
        .inputs()
        .filter(|port| port.spec.coherent)
        .flat_map(|port| &port.feeds)
        .map(|feed| feed.source.as_str())
        .collect();
    sources.iter().enumerate().any(|(index, source)| {
        sources[..index].contains(source) && unclocked(source, devices, live)
    })
}

fn unclocked(source: &str, devices: &[(String, u32)], live: &StateSnapshot) -> bool {
    devices
        .iter()
        .find(|(node, _)| node == source)
        .and_then(|(_, id)| live.device_sets.iter().find(|set| set.id == *id))
        .is_some_and(|set| set.capabilities.coherence == Coherence::None)
}

fn radio_runs(
    graph: &PatchGraph,
    id: &str,
    (devices, attached): (&[(String, u32)], &[DeviceInfo]),
    problems: &mut Vec<NodeProblem>,
) -> Option<Runs> {
    let node = graph.node(id)?;
    if !node.body.opens_device() {
        return None;
    }
    let Some(reference) = node.body.device_ref(id) else {
        return Some(unbound(UnboundReason::NoRadio));
    };
    if let Some((_, device_set)) = devices.iter().find(|(bound, _)| bound == id) {
        return Some(Runs::Radio {
            device_set: *device_set,
        });
    }
    let present = !matches!(node.body, NodeBody::Device(_))
        || attached.iter().any(|device| reference.matches(device));
    let reason = if present {
        UnboundReason::RadioClosed
    } else {
        UnboundReason::RadioAbsent
    };
    problems.push(problem(id, reason.text()));
    Some(unbound(reason))
}

fn tap_lanes(
    node: &mut ResolvedNode,
    graph: &PatchGraph,
    devices: &[(String, u32)],
    live: &StateSnapshot,
) {
    for port in node
        .ports
        .iter_mut()
        .filter(|port| port.spec.port_type == PortType::Iq)
    {
        for feed in &mut port.feeds {
            feed.tap = lane_of(graph, feed, devices, live);
        }
    }
}

fn lane_of(
    graph: &PatchGraph,
    feed: &Feed,
    devices: &[(String, u32)],
    live: &StateSnapshot,
) -> Option<Tap> {
    let body = &graph.node(&feed.source)?.body;
    if body.opens_device() {
        let device_set = devices
            .iter()
            .find(|(node, _)| *node == feed.source)
            .map(|(_, set)| *set)?;
        let stream = feed.stream.unwrap_or_default();
        return Some(Tap::Lane { device_set, stream });
    }
    live.device_sets.iter().find_map(|set| {
        set.virtual_lanes
            .iter()
            .find(|lane| lane.node == feed.source && lane.port == feed.port)
            .map(|lane| Tap::Lane {
                device_set: set.id,
                stream: lane.stream,
            })
    })
}

fn channel_runs(
    graph: &PatchGraph,
    node: &ResolvedNode,
    (live, radios): (&StateSnapshot, &[(String, Runs)]),
    problems: &mut Vec<NodeProblem>,
) -> Option<Runs> {
    match &graph.node(&node.id)?.body {
        NodeBody::Channel(channel) => {
            let feeds: &[Feed] = node.input(IQ_PORT).map_or(&[], |port| &port.feeds);
            let lanes: Vec<(u32, u32)> = feeds
                .iter()
                .filter_map(|feed| feed.tap.as_ref()?.lane())
                .collect();
            if lanes.is_empty() {
                return Some(unbound(unwired_reason(feeds, radios)));
            }
            let placed = live
                .device_sets
                .iter()
                .find_map(|set| placed_on(set, &node.id, &channel.channel_type, &lanes));
            Some(placed.unwrap_or_else(|| {
                problems.push(problem(&node.id, UnboundReason::NotPlaced.text()));
                unbound(UnboundReason::NotPlaced)
            }))
        }
        NodeBody::DmrTrunk(_) => Some(
            live.trunk_systems
                .iter()
                .find(|system| system.node == node.id)
                .map_or_else(
                    || unbound(UnboundReason::NotRunning),
                    |system| Runs::Trunk {
                        control: system.control.as_ref().map(|control| ChannelAddr {
                            device_set: control.device_set,
                            channel: control.channel,
                        }),
                        followers: system
                            .followers
                            .iter()
                            .map(|follower| ChannelAddr {
                                device_set: follower.device_set,
                                channel: follower.channel,
                            })
                            .collect(),
                    },
                ),
        ),
        _ => None,
    }
}

fn unwired_reason(feeds: &[Feed], radios: &[(String, Runs)]) -> UnboundReason {
    if feeds.is_empty() {
        return UnboundReason::NotWired;
    }
    feeds
        .iter()
        .find_map(|feed| match runs_at(radios, &feed.source) {
            Some(Runs::Unbound { reason }) => Some(*reason),
            _ => None,
        })
        .unwrap_or(UnboundReason::NotRunning)
}

fn placed_on(
    set: &DeviceSet,
    node: &str,
    channel_type: &str,
    lanes: &[(u32, u32)],
) -> Option<Runs> {
    set.channels
        .iter()
        .find(|live| {
            live.node.as_deref() == Some(node)
                && live.settings.params.type_id() == channel_type
                && lanes.contains(&(set.id, live.stream))
        })
        .map(|live| Runs::Channel {
            device_set: set.id,
            channel: live.id,
            stream: live.stream,
        })
}

fn tap_channels(node: &mut ResolvedNode, runs: &[(String, Runs)]) {
    for port in node
        .ports
        .iter_mut()
        .filter(|port| port.spec.port_type != PortType::Iq)
    {
        for feed in &mut port.feeds {
            feed.tap = runs
                .iter()
                .find(|(id, _)| *id == feed.source)
                .and_then(|(_, runs)| channel_tap(runs));
        }
    }
}

fn channel_tap(runs: &Runs) -> Option<Tap> {
    match runs {
        Runs::Channel {
            device_set,
            channel,
            ..
        } => Some(Tap::Channel {
            device_set: *device_set,
            channel: *channel,
        }),
        Runs::Trunk { control, followers } => Some(Tap::Channels {
            channels: control.iter().chain(followers).copied().collect(),
        }),
        Runs::Unbound { .. } | Runs::Radio { .. } | Runs::Engine | Runs::Server => None,
    }
}

#[must_use]
pub fn lane_follows(patch: &ResolvedPatch, graph: &PatchGraph, channel: &str) -> bool {
    let Some(NodeBody::Channel(node)) = graph.node(channel).map(|held| &held.body) else {
        return false;
    };
    let [feed] = patch.feeds(channel, IQ_PORT) else {
        return false;
    };
    let Some((_, stream)) = feed.tap.as_ref().and_then(Tap::lane) else {
        return false;
    };
    if node.tuning_locked {
        return false;
    }
    match graph.node(&feed.source).map(|held| &held.body) {
        Some(NodeBody::Device(device)) => !device.tuning_locked(stream),
        Some(body) if body.opens_device() => true,
        Some(_) => patch.first_feed(&feed.source, ARRAY_PORT).is_some(),
        None => false,
    }
}

const fn unbound(reason: UnboundReason) -> Runs {
    Runs::Unbound { reason }
}

fn problem(node: &str, text: &str) -> NodeProblem {
    NodeProblem {
        node: node.to_owned(),
        source: ProblemSource::Bind,
        text: text.to_owned(),
    }
}

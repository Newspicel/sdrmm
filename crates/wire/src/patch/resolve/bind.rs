use super::{
    super::{IQ_PORT, NodeBody, PatchGraph, PortType, Refusal, port_stream},
    ChannelAddr, NodeProblem, ProblemSource, ResolvedEdge, ResolvedNode, ResolvedPatch, Runs, Tap,
};
use crate::{
    device::Coherence,
    state::{DeviceSet, StateSnapshot},
};

pub const NO_RADIO: &str = "No radio named";
pub const RADIO_ABSENT: &str = "Radio not open";
pub const NOT_WIRED: &str = "No radio wired";
pub const NOT_PLACED: &str = "Not placed on its radio";
pub const NOT_RUNNING: &str = "Not running";

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
) -> Vec<NodeProblem> {
    let devices = bound_devices(graph, live);
    let mut problems = Vec::new();
    for node in &mut patch.nodes {
        if let Some(runs) = radio_runs(graph, &node.id, &devices, &mut problems) {
            node.runs = runs;
        }
    }
    for node in &mut patch.nodes {
        tap_lanes(node, graph, &devices, live);
    }
    for node in &mut patch.nodes {
        if let Some(runs) = channel_runs(graph, node, live, &mut problems) {
            node.runs = runs;
        }
    }
    let channels: Vec<(String, Runs)> = patch
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node.runs.clone()))
        .collect();
    for node in &mut patch.nodes {
        tap_channels(node, &channels);
    }
    for edge in &mut patch.edges {
        edge.retunes_source = retunes(edge, graph, &devices, live);
    }
    problems.extend(
        patch
            .nodes
            .iter()
            .filter(|node| shares_no_clock(node, &devices, live))
            .map(|node| problem(&node.id, &Refusal::SharedClock.text())),
    );
    problems
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
    devices: &[(String, u32)],
    problems: &mut Vec<NodeProblem>,
) -> Option<Runs> {
    let node = graph.node(id)?;
    if !node.body.opens_device() {
        return None;
    }
    if node.body.device_ref(id).is_none() {
        return Some(unbound(NO_RADIO));
    }
    match devices.iter().find(|(bound, _)| bound == id) {
        Some((_, device_set)) => Some(Runs::Radio {
            device_set: *device_set,
        }),
        None => {
            problems.push(problem(id, RADIO_ABSENT));
            Some(unbound(RADIO_ABSENT))
        }
    }
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
            feed.tap = lane_of(graph, &feed.source, &feed.port, devices, live);
        }
    }
}

fn lane_of(
    graph: &PatchGraph,
    source: &str,
    port: &str,
    devices: &[(String, u32)],
    live: &StateSnapshot,
) -> Option<Tap> {
    let body = &graph.node(source)?.body;
    if body.opens_device() {
        let device_set = devices
            .iter()
            .find(|(node, _)| node == source)
            .map(|(_, set)| *set)?;
        let stream = port_stream(IQ_PORT, port)?;
        return Some(Tap::Lane { device_set, stream });
    }
    live.device_sets.iter().find_map(|set| {
        set.virtual_lanes
            .iter()
            .find(|lane| lane.node == source && lane.port == port)
            .map(|lane| Tap::Lane {
                device_set: set.id,
                stream: lane.stream,
            })
    })
}

fn channel_runs(
    graph: &PatchGraph,
    node: &ResolvedNode,
    live: &StateSnapshot,
    problems: &mut Vec<NodeProblem>,
) -> Option<Runs> {
    match &graph.node(&node.id)?.body {
        NodeBody::Channel(channel) => {
            let lanes: Vec<(u32, u32)> = node
                .input(IQ_PORT)
                .into_iter()
                .flat_map(|port| &port.feeds)
                .filter_map(|feed| feed.tap.as_ref()?.lane())
                .collect();
            if lanes.is_empty() {
                return Some(unbound(NOT_WIRED));
            }
            let placed = live
                .device_sets
                .iter()
                .find_map(|set| placed_on(set, &node.id, &channel.channel_type, &lanes));
            Some(placed.unwrap_or_else(|| {
                problems.push(problem(&node.id, NOT_PLACED));
                unbound(NOT_PLACED)
            }))
        }
        NodeBody::DmrTrunk(_) => Some(
            live.trunk_systems
                .iter()
                .find(|system| system.node == node.id)
                .map_or_else(
                    || unbound(NOT_RUNNING),
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

fn retunes(
    edge: &ResolvedEdge,
    graph: &PatchGraph,
    devices: &[(String, u32)],
    live: &StateSnapshot,
) -> bool {
    let (Some(source), Some(target)) = (graph.node(&edge.from.node), graph.node(&edge.to.node))
    else {
        return false;
    };
    let (NodeBody::Device(device), NodeBody::Channel(channel)) = (&source.body, &target.body)
    else {
        return false;
    };
    let Some(stream) = port_stream(IQ_PORT, &edge.from.port) else {
        return false;
    };
    if edge.to.port != IQ_PORT || channel.tuning_locked || device.tuning_locked(stream) {
        return false;
    }
    devices
        .iter()
        .find(|(node, _)| *node == source.id)
        .and_then(|(_, id)| live.device_sets.iter().find(|set| set.id == *id))
        .is_none_or(|set| {
            set.settings
                .for_stream(stream, &set.capabilities.per_stream)
                .tunes_itself()
        })
}

fn unbound(reason: &str) -> Runs {
    Runs::Unbound {
        reason: reason.to_owned(),
    }
}

fn problem(node: &str, text: &str) -> NodeProblem {
    NodeProblem {
        node: node.to_owned(),
        source: ProblemSource::Bind,
        text: text.to_owned(),
    }
}

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{
    CONTROL_PORT, NodeBody, NodeKind, PatchEdge, PatchError, PatchGraph, PatchNode, PortDirection,
    PortRef, PortSpec, Role,
};
use crate::{
    channel::ChannelDescriptor,
    control::ControlVerb,
    decode::{EventKind, KindSet},
    device::{Capabilities, Coherence},
    patch::{Keeps, Payload},
    state::StateSnapshot,
};

mod bind;
mod checks;
mod feeds;
mod offer;
mod ports;
mod wiring;

pub use bind::{bind, bound_devices};
pub use wiring::{WireVerdict, WiringRequest, wiring};

#[derive(Clone, Copy, Debug, Default)]
pub struct ResolveCtx<'a> {
    pub descriptors: Option<&'a [ChannelDescriptor]>,
    pub live: Option<&'a StateSnapshot>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ResolvedPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<i64>,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub epoch: u64,
    pub nodes: Vec<ResolvedNode>,
    pub edges: Vec<ResolvedEdge>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ResolvedNode {
    pub id: String,
    pub kind: NodeKind,
    pub label: String,
    pub ports: Vec<ResolvedPort>,
    pub runs: Runs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub array_offer: Option<ArrayOffer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<Held>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum Held {
    Hand,
    Wire { node: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "offer", rename_all = "snake_case")]
pub enum ArrayOffer {
    Ready { wires: Vec<LaneWire> },
    Refused { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct LaneWire {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ResolvedPort {
    #[serde(flatten)]
    pub spec: PortSpec,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feeds: Vec<Feed>,
    #[serde(default, skip_serializing_if = "no_kinds")]
    pub ignored: KindSet,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reaches: Vec<Reach>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kept: Vec<Keeps>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Feed {
    pub source: String,
    pub port: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<String>,
    pub payload: Payload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tap: Option<Tap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drives: Option<ControlVerb>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub struct ChannelAddr {
    pub device_set: u32,
    pub channel: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "tap", rename_all = "snake_case")]
pub enum Tap {
    Lane { device_set: u32, stream: u32 },
    Channel { device_set: u32, channel: u32 },
    Channels { channels: Vec<ChannelAddr> },
}

impl Tap {
    #[must_use]
    pub fn lane(&self) -> Option<(u32, u32)> {
        match self {
            Self::Lane { device_set, stream } => Some((*device_set, *stream)),
            Self::Channel { .. } | Self::Channels { .. } => None,
        }
    }

    #[must_use]
    pub fn channels(&self) -> Vec<ChannelAddr> {
        match self {
            Self::Lane { .. } => Vec::new(),
            Self::Channel {
                device_set,
                channel,
            } => vec![ChannelAddr {
                device_set: *device_set,
                channel: *channel,
            }],
            Self::Channels { channels } => channels.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Reach {
    pub sink: String,
    pub port: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<String>,
    #[serde(default, skip_serializing_if = "no_kinds")]
    pub kinds: KindSet,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_runs: Option<Runs>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RefusedEdge {
    pub edge: PatchEdge,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UnboundReason {
    NotWired,
    NoRadio,
    RadioAbsent,
    RadioClosed,
    NotPlaced,
    NotRunning,
}

impl UnboundReason {
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::NotWired => "No radio wired",
            Self::NoRadio => "No radio named",
            Self::RadioAbsent => "Radio not connected",
            Self::RadioClosed => "Radio not open",
            Self::NotPlaced => "Not placed on its radio",
            Self::NotRunning => "Not running",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "runs", rename_all = "snake_case")]
pub enum Runs {
    Unbound {
        reason: UnboundReason,
    },
    Radio {
        device_set: u32,
    },
    Channel {
        device_set: u32,
        channel: u32,
        stream: u32,
    },
    Trunk {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        control: Option<ChannelAddr>,
        followers: Vec<ChannelAddr>,
    },
    Engine,
    Server,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ResolvedEdge {
    pub from: PortRef,
    pub to: PortRef,
    #[serde(default)]
    pub retunes_source: bool,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ProblemSource {
    Bind,
    Position,
    Monitor,
    Apply,
    Calls,
    Recorder,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NodeProblem {
    pub node: String,
    pub source: ProblemSource,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ResolvedState {
    pub patch: ResolvedPatch,
    pub problems: Vec<NodeProblem>,
}

fn no_kinds(kinds: &KindSet) -> bool {
    kinds.is_empty()
}

pub fn compile(graph: &PatchGraph, ctx: &ResolveCtx<'_>) -> Result<ResolvedPatch, PatchError> {
    checks::structure(graph, ctx.descriptors)?;
    let (patch, refused) = draft(graph, ctx);
    match refused.into_iter().next() {
        Some((_, error)) => Err(error),
        None => Ok(patch),
    }
}

#[must_use]
pub fn compile_draft(
    graph: &PatchGraph,
    ctx: &ResolveCtx<'_>,
) -> (ResolvedPatch, Vec<RefusedEdge>) {
    let (patch, refused) = draft(graph, ctx);
    let refused = refused
        .into_iter()
        .map(|(edge, error)| RefusedEdge {
            edge,
            reason: error.to_string(),
        })
        .collect();
    (patch, refused)
}

fn draft(
    graph: &PatchGraph,
    ctx: &ResolveCtx<'_>,
) -> (ResolvedPatch, Vec<(PatchEdge, PatchError)>) {
    let mut net = Net::new(graph, ctx);
    let refused = checks::wire(&mut net);
    (assemble(&net), refused)
}

pub(super) struct Net<'a> {
    graph: &'a PatchGraph,
    edges: Vec<&'a PatchEdge>,
    ports: Vec<Vec<PortSpec>>,
    coherence: Vec<Option<Coherence>>,
}

impl<'a> Net<'a> {
    fn new(graph: &'a PatchGraph, ctx: &ResolveCtx<'_>) -> Self {
        let devices = ctx
            .live
            .map(|live| bound_devices(graph, live))
            .unwrap_or_default();
        let caps = |node: &PatchNode| -> Option<&Capabilities> {
            let (_, set) = devices.iter().find(|(id, _)| *id == node.id)?;
            ctx.live?
                .device_sets
                .iter()
                .find(|open| open.id == *set)
                .map(|open| &open.capabilities)
        };
        Self {
            graph,
            edges: Vec::with_capacity(graph.edges.len()),
            ports: graph
                .nodes
                .iter()
                .map(|node| ports::concrete(graph, node, ctx.descriptors, caps(node)))
                .collect(),
            coherence: graph
                .nodes
                .iter()
                .map(|node| caps(node).map(|caps| caps.coherence))
                .collect(),
        }
    }

    fn node(&self, id: &str) -> Option<(&'a PatchNode, &[PortSpec])> {
        let index = self.graph.nodes.iter().position(|node| node.id == id)?;
        Some((&self.graph.nodes[index], &self.ports[index]))
    }

    fn ports_of(&self, id: &str) -> &[PortSpec] {
        self.node(id).map_or(&[], |(_, ports)| ports)
    }

    fn spec(&self, reference: &PortRef, direction: PortDirection) -> Option<&PortSpec> {
        ports::find(self.ports_of(&reference.node), reference, direction).ok()
    }

    fn label(&self, id: &str) -> String {
        self.node(id).map_or_else(
            || id.to_owned(),
            |(node, _)| {
                node.label
                    .clone()
                    .unwrap_or_else(|| node.body.kind().name().to_owned())
            },
        )
    }
}

fn assemble(net: &Net<'_>) -> ResolvedPatch {
    let nodes = net
        .graph
        .nodes
        .iter()
        .zip(&net.ports)
        .map(|(node, specs)| {
            let ports: Vec<ResolvedPort> = specs
                .iter()
                .map(|spec| feeds::resolve_port(net, &node.id, spec.clone()))
                .collect();
            ResolvedNode {
                id: node.id.clone(),
                kind: node.body.kind(),
                label: net.label(&node.id),
                held: held(&node.body, &ports),
                ports,
                runs: initial_runs(&node.body),
                array_offer: None,
            }
        })
        .collect();
    ResolvedPatch {
        nodes,
        edges: net
            .edges
            .iter()
            .map(|edge| ResolvedEdge {
                from: edge.from.clone(),
                to: edge.to.clone(),
                retunes_source: false,
            })
            .collect(),
        ..ResolvedPatch::default()
    }
}

fn held(body: &NodeBody, ports: &[ResolvedPort]) -> Option<Held> {
    let NodeBody::Channel(channel) = body else {
        return None;
    };
    if channel.tuning_locked {
        return Some(Held::Hand);
    }
    ports
        .iter()
        .filter(|port| port.spec.direction == PortDirection::In && port.spec.name == CONTROL_PORT)
        .flat_map(|port| &port.feeds)
        .next()
        .map(|feed| Held::Wire {
            node: feed.source.clone(),
        })
}

fn initial_runs(body: &NodeBody) -> Runs {
    match body.role() {
        Role::OpensRadio => Runs::Unbound {
            reason: UnboundReason::NoRadio,
        },
        Role::Processor => Runs::Engine,
        Role::Pass(_) | Role::Plain => match body {
            NodeBody::Array(_) => Runs::Engine,
            NodeBody::Channel(_) | NodeBody::DmrTrunk(_) => Runs::Unbound {
                reason: UnboundReason::NotWired,
            },
            _ => Runs::Server,
        },
    }
}

impl ResolvedPatch {
    #[must_use]
    pub fn node(&self, id: &str) -> Option<&ResolvedNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn nodes_of(&self, kind: NodeKind) -> impl Iterator<Item = &ResolvedNode> {
        self.nodes.iter().filter(move |node| node.kind == kind)
    }

    #[must_use]
    pub fn feeds(&self, node: &str, port: &str) -> &[Feed] {
        self.node(node)
            .and_then(|found| found.input(port))
            .map_or(&[], |found| found.feeds.as_slice())
    }

    #[must_use]
    pub fn reaches(&self, node: &str, port: &str) -> &[Reach] {
        self.node(node)
            .and_then(|found| found.output(port))
            .map_or(&[], |found| found.reaches.as_slice())
    }

    #[must_use]
    pub fn keeps(&self, node: &str, port: &str, keeps: Keeps) -> bool {
        self.node(node)
            .and_then(|found| found.output(port))
            .is_some_and(|found| found.kept.contains(&keeps))
    }

    #[must_use]
    pub fn takes_kept(&self, node: &str, port: &str, keeps: Keeps) -> bool {
        self.node(node)
            .and_then(|found| found.input(port))
            .is_some_and(|found| found.spec.keeps.contains(&keeps))
    }

    #[must_use]
    pub fn delivers(&self, node: &str, port: &str, kind: EventKind) -> bool {
        self.reaches(node, port)
            .iter()
            .any(|reach| reach.kinds.contains(kind))
    }

    #[must_use]
    pub fn retunes_source(&self, node: &str, port: &str) -> bool {
        self.edges
            .iter()
            .any(|edge| edge.to.node == node && edge.to.port == port && edge.retunes_source)
    }

    #[must_use]
    pub fn first_feed(&self, node: &str, port: &str) -> Option<&Feed> {
        self.feeds(node, port).first()
    }

    #[must_use]
    pub fn runs(&self, node: &str) -> Option<&Runs> {
        self.node(node).map(|found| &found.runs)
    }

    #[must_use]
    pub fn device_set(&self, node: &str) -> Option<u32> {
        match self.runs(node)? {
            Runs::Radio { device_set } => Some(*device_set),
            _ => None,
        }
    }

    #[must_use]
    pub fn channel(&self, node: &str) -> Option<ChannelAddr> {
        match self.runs(node)? {
            Runs::Channel {
                device_set,
                channel,
                ..
            } => Some(ChannelAddr {
                device_set: *device_set,
                channel: *channel,
            }),
            _ => None,
        }
    }

    #[must_use]
    pub fn radios(&self) -> Vec<(String, u32)> {
        self.nodes
            .iter()
            .filter_map(|node| Some((node.id.clone(), self.device_set(&node.id)?)))
            .collect()
    }

    #[must_use]
    pub fn channels(&self) -> Vec<(String, ChannelAddr)> {
        self.nodes
            .iter()
            .filter_map(|node| Some((node.id.clone(), self.channel(&node.id)?)))
            .collect()
    }

    #[must_use]
    pub fn owner_of(&self, addr: ChannelAddr) -> Option<&str> {
        self.nodes
            .iter()
            .find(|node| match &node.runs {
                Runs::Channel {
                    device_set,
                    channel,
                    ..
                } => addr.device_set == *device_set && addr.channel == *channel,
                Runs::Trunk { control, followers } => {
                    *control == Some(addr) || followers.contains(&addr)
                }
                _ => false,
            })
            .map(|node| node.id.as_str())
    }

    pub fn may_tune(&self, node: &str, actor: Option<&str>) -> Result<(), String> {
        match self.node(node).and_then(|found| found.held.as_ref()) {
            None => Ok(()),
            Some(Held::Wire { node: by }) if actor == Some(by.as_str()) => Ok(()),
            Some(Held::Hand) => Err("Tuning locked".to_owned()),
            Some(Held::Wire { node: by }) => Err(format!(
                "Tuned by {}",
                self.node(by)
                    .map_or(by.as_str(), |found| found.label.as_str())
            )),
        }
    }

    #[must_use]
    pub fn same_binding(&self, other: &Self) -> bool {
        self.workspace == other.workspace
            && self.revision == other.revision
            && self.nodes == other.nodes
            && self.edges == other.edges
    }
}

impl ResolvedNode {
    #[must_use]
    pub fn input(&self, name: &str) -> Option<&ResolvedPort> {
        self.port(name, PortDirection::In)
    }

    #[must_use]
    pub fn output(&self, name: &str) -> Option<&ResolvedPort> {
        self.port(name, PortDirection::Out)
    }

    fn port(&self, name: &str, direction: PortDirection) -> Option<&ResolvedPort> {
        self.ports
            .iter()
            .find(|port| port.spec.direction == direction && port.spec.name == name)
    }

    pub fn inputs(&self) -> impl Iterator<Item = &ResolvedPort> {
        self.ports
            .iter()
            .filter(|port| port.spec.direction == PortDirection::In)
    }

    pub fn outputs(&self) -> impl Iterator<Item = &ResolvedPort> {
        self.ports
            .iter()
            .filter(|port| port.spec.direction == PortDirection::Out)
    }
}

#[cfg(test)]
mod tests;

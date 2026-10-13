use std::collections::HashMap;

use sdrmm_engine::Engine;
use sdrmm_wire::{
    ChannelAddr, IQ_PORT, NodeKind, PortType, PositionFix, ProblemSource, ResolvedNode,
    ResolvedPatch, StateSnapshot,
};

use crate::problems::ProblemBoard;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PositionTarget {
    Decoder(ChannelAddr),
    IqRecording { device_set: u32 },
    IqHistory { device_set: u32 },
    Sink { node: String },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PositionRoutes {
    by_source: HashMap<String, Vec<(String, PositionTarget)>>,
}

impl PositionRoutes {
    pub(crate) fn resolve(patch: &ResolvedPatch) -> Self {
        let mut routes = Self::default();
        for node in &patch.nodes {
            let sources: Vec<&str> = node
                .inputs()
                .filter(|port| port.spec.port_type == PortType::Position)
                .flat_map(|port| &port.feeds)
                .map(|feed| feed.source.as_str())
                .collect();
            let Some(target) = target_of(patch, node) else {
                continue;
            };
            for source in sources {
                routes
                    .by_source
                    .entry(source.to_owned())
                    .or_default()
                    .push((node.id.clone(), target.clone()));
            }
        }
        routes
    }

    pub(crate) fn targets(&self, source: &str) -> &[(String, PositionTarget)] {
        self.by_source.get(source).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn reaches(&self, node: &str) -> bool {
        self.by_source
            .values()
            .flatten()
            .any(|(target, _)| target == node)
    }
}

fn target_of(patch: &ResolvedPatch, node: &ResolvedNode) -> Option<PositionTarget> {
    let lane = || {
        patch
            .feeds(&node.id, IQ_PORT)
            .iter()
            .find_map(|feed| feed.tap.as_ref()?.lane())
            .map(|(device_set, _)| device_set)
    };
    Some(match node.kind {
        NodeKind::Channel => PositionTarget::Decoder(patch.channel(&node.id)?),
        NodeKind::Recorder => PositionTarget::IqRecording {
            device_set: lane()?,
        },
        NodeKind::TimeMachine => PositionTarget::IqHistory {
            device_set: lane()?,
        },
        _ => PositionTarget::Sink {
            node: node.id.clone(),
        },
    })
}

pub(crate) fn push(
    engine: &Engine,
    problems: &ProblemBoard,
    routes: &PositionRoutes,
    source: &str,
    fix: Option<&PositionFix>,
) {
    let mut live: Option<StateSnapshot> = None;
    for (node, target) in routes.targets(source) {
        let result = match target {
            PositionTarget::Decoder(addr) => engine
                .update_channel_position(addr.device_set, addr.channel, fix.cloned())
                .map_err(|error| error.to_string()),
            PositionTarget::IqRecording { device_set } => {
                let live = live.get_or_insert_with(|| engine.snapshot());
                if live
                    .device_sets
                    .iter()
                    .any(|set| set.id == *device_set && set.recording.is_some())
                {
                    engine
                        .update_recording_position(*device_set, fix.cloned())
                        .map_err(|error| error.to_string())
                } else {
                    Ok(())
                }
            }
            PositionTarget::IqHistory { device_set } => {
                let live = live.get_or_insert_with(|| engine.snapshot());
                if live
                    .device_sets
                    .iter()
                    .any(|set| set.id == *device_set && set.time_machine.is_some())
                {
                    engine
                        .update_time_machine_position(*device_set, fix.cloned())
                        .map_err(|error| error.to_string())
                } else {
                    Ok(())
                }
            }
            PositionTarget::Sink { .. } => Ok(()),
        };
        problems.set(
            engine,
            ProblemSource::Position,
            node,
            result.err().into_iter().collect(),
        );
    }
}

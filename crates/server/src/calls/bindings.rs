use sdrmm_wire::{
    CallGate, ChannelAddr, EVENTS_PORT, EventKind, Keeps, NodeProblem, ProblemSource,
    ResolvedPatch, Runs, StateSnapshot,
};

use super::tracker::Bindings;

const SQUELCH_OFF: &str = "Squelch off, no calls";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CallBinding {
    pub node: String,
    pub device_set: u32,
    pub channel: u32,
    pub gate: CallGate,
    pub keep_audio: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum GateMiss {
    SquelchOff,
    Unknown,
}

pub(crate) fn bindings(
    patch: &ResolvedPatch,
    live: &StateSnapshot,
) -> (Bindings, Vec<NodeProblem>) {
    let mut bound = Bindings::new();
    let mut problems = Vec::new();
    for node in &patch.nodes {
        if !patch.delivers(&node.id, EVENTS_PORT, EventKind::Call) {
            continue;
        }
        let keep_audio = patch.keeps(&node.id, EVENTS_PORT, Keeps::Clips);
        let mut silent = false;
        for addr in addresses(&node.runs) {
            match gate_of(live, addr) {
                Ok(gate) => bound
                    .entry((addr.device_set, addr.channel))
                    .or_default()
                    .push(CallBinding {
                        node: node.id.clone(),
                        device_set: addr.device_set,
                        channel: addr.channel,
                        gate,
                        keep_audio,
                    }),
                Err(GateMiss::SquelchOff) => silent = true,
                Err(GateMiss::Unknown) => {}
            }
        }
        if silent {
            problems.push(NodeProblem {
                node: node.id.clone(),
                source: ProblemSource::Calls,
                text: SQUELCH_OFF.to_owned(),
            });
        }
    }
    (bound, problems)
}

fn addresses(runs: &Runs) -> Vec<ChannelAddr> {
    match runs {
        Runs::Channel {
            device_set,
            channel,
            ..
        } => vec![ChannelAddr {
            device_set: *device_set,
            channel: *channel,
        }],
        Runs::Trunk { followers, .. } => followers.clone(),
        Runs::Unbound { .. } | Runs::Radio { .. } | Runs::Engine | Runs::Server => Vec::new(),
    }
}

fn gate_of(live: &StateSnapshot, addr: ChannelAddr) -> Result<CallGate, GateMiss> {
    let info = live
        .device_sets
        .iter()
        .find(|set| set.id == addr.device_set)
        .and_then(|set| set.channels.iter().find(|info| info.id == addr.channel))
        .ok_or(GateMiss::Unknown)?;
    let gate = sdrmm_channels::descriptor(info.settings.params.type_id())
        .and_then(|descriptor| descriptor.calls)
        .ok_or(GateMiss::Unknown)?;
    match gate {
        CallGate::Squelch if info.settings.squelch.is_off() => Err(GateMiss::SquelchOff),
        gate => Ok(gate),
    }
}

#[cfg(test)]
mod tests;

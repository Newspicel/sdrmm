use std::sync::atomic::Ordering;

use sdrmm_wire::{
    TrunkChannel, TrunkChannelSource, TrunkControl, TrunkFollower, TrunkProbe, TrunkProblem,
    TrunkSystemStatus,
};

use super::{Follower, TrunkSystem, prospect::Prospector};

impl Follower {
    pub(super) fn publish(&self) {
        let statuses = self
            .systems
            .iter()
            .map(|system| self.status_of(system))
            .collect();
        *self
            .status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = statuses;
    }

    pub(super) fn status_of(&self, system: &TrunkSystem) -> TrunkSystemStatus {
        let mut status = TrunkSystemStatus {
            node: system.node.clone(),
            detected: self.detected.get(&system.node).cloned(),
            carriers: self
                .carriers
                .values()
                .filter(|carrier| carrier.system_node == system.node)
                .count() as u32,
            control: self.controls.get(&system.node).map(|control| TrunkControl {
                device_set: control.device_set,
                channel: control.channel,
                freq_hz: control.freq_hz,
            }),
            followers: self
                .followers
                .iter()
                .filter(|(key, _)| key.system_node() == system.node)
                .map(|(key, follower)| TrunkFollower {
                    device_set: follower.device_set,
                    channel: follower.channel,
                    logical_channel: key.logical_channel(),
                    slot: key.slot(),
                    freq_hz: follower.freq_hz,
                    destination: follower.call.map(|call| call.destination),
                    source: follower.call.and_then(|call| call.source),
                    group_call: follower.call.and_then(|call| call.group_call),
                })
                .collect(),
            problems: self
                .problems
                .iter()
                .filter(|(key, _)| key.system_node() == system.node)
                .map(|(key, problem)| TrunkProblem {
                    freq_hz: problem.freq_hz,
                    slot: key.slot(),
                    logical_channel: key.logical_channel(),
                    reason: problem.reason.clone(),
                    since: problem.since.clone(),
                    attempts: problem.attempts,
                })
                .collect(),
            channel_map: self.channel_map_of(&system.node),
            probes: self
                .probes
                .iter()
                .filter(|(_, probe)| probe.system_node == system.node)
                .map(|((device_set, channel), probe)| TrunkProbe {
                    device_set: *device_set,
                    channel: *channel,
                    freq_hz: probe.freq_hz,
                })
                .collect(),
            searching: self
                .prospectors
                .get(&system.node)
                .map_or(0, Prospector::searching),
            candidates: self
                .prospectors
                .get(&system.node)
                .map_or(0, Prospector::candidates),
            other_control_hz: self
                .prospectors
                .get(&system.node)
                .map(Prospector::other_control_hz)
                .unwrap_or_default(),
            color_code: self
                .prospectors
                .get(&system.node)
                .and_then(Prospector::color_code),
            site: self.sites.get(&system.node).copied().unwrap_or_default(),
            dropped: self.dropped.load(Ordering::Relaxed),
        };
        status
            .followers
            .sort_by_key(|follower| (follower.freq_hz, follower.slot));
        status
            .problems
            .sort_by_key(|problem| (problem.freq_hz, problem.slot));
        status.probes.sort_by_key(|probe| probe.freq_hz);
        status
    }

    pub(super) fn channel_map_of(&self, system_node: &str) -> Vec<TrunkChannel> {
        let mut map: Vec<TrunkChannel> = self
            .definitions
            .iter()
            .filter(|((node, _), _)| node == system_node)
            .map(|((_, logical_channel), freq_hz)| TrunkChannel {
                logical_channel: *logical_channel,
                freq_hz: *freq_hz,
                source: TrunkChannelSource::Announced,
                confidence: 100,
            })
            .collect();
        if let Some(prospector) = self.prospectors.get(system_node) {
            map.extend(prospector.channel_map().into_iter().filter(|channel| {
                !self
                    .definitions
                    .contains_key(&(system_node.to_owned(), channel.logical_channel))
            }));
        }
        map.sort_unstable_by_key(|channel| channel.logical_channel);
        map
    }
}

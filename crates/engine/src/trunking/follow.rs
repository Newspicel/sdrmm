use std::time::Instant;

use sdrmm_channels::TrunkGrant;
use sdrmm_wire::{ChannelSettings, DvFrame, DvFrameKind, StateScope};

use super::{
    Carrier, Follower, FollowerChannel, FollowerKey, Grant, MAX_FOLLOWERS_PER_SYSTEM, Problem,
    RETRY_BASE, RETRY_MAX, prospect::Prospector,
};
use crate::Engine;

impl Follower {
    /// Reads a burst through the protocol's own trunking: where it places logical channels and
    /// which calls it grants.
    pub(super) fn observe_grants(
        &mut self,
        engine: &Engine,
        carrier: &Carrier,
        frame: &DvFrame,
    ) -> bool {
        if let Some(color_code) = frame.color_code
            && let Some(prospector) = self.prospectors.get_mut(&carrier.system_node)
        {
            prospector.note_site(color_code);
        }
        for placement in (carrier.trunking.placements)(frame) {
            self.definitions.insert(
                (carrier.system_node.clone(), placement.logical_channel),
                placement.freq_hz,
            );
        }
        if frame.kind != DvFrameKind::Control {
            return false;
        }
        let mut woke = false;
        for call in (carrier.trunking.grants)(frame) {
            woke |= self.follow_granted(engine, carrier, call);
        }
        woke
    }

    pub(super) fn follow_granted(
        &mut self,
        engine: &Engine,
        carrier: &Carrier,
        call: TrunkGrant,
    ) -> bool {
        if call.group_call != Some(false)
            && self
                .talkgroups_of(&carrier.system_node)
                .is_some_and(|talkgroups| !talkgroups.follows(call.destination))
        {
            return false;
        }
        let announced = self
            .definitions
            .get(&(carrier.system_node.clone(), call.logical_channel))
            .copied();
        let Some(freq_hz) = announced.or_else(|| {
            self.prospectors
                .get(&carrier.system_node)
                .and_then(|prospector| prospector.frequency_of(call.logical_channel))
        }) else {
            return self
                .prospectors
                .get_mut(&carrier.system_node)
                .is_some_and(|prospector| {
                    prospector.note_grant(
                        call.logical_channel,
                        call.slot,
                        call.destination,
                        call.source,
                        Instant::now(),
                    )
                });
        };
        self.ensure_follower(
            engine,
            carrier,
            FollowerKey::Granted {
                system_node: carrier.system_node.clone(),
                logical_channel: call.logical_channel,
                slot: call.slot,
            },
            Grant {
                slot: call.slot,
                freq_hz,
                call: Some(call),
            },
        );
        false
    }

    pub(super) fn provision_known_carrier(&mut self, engine: &Engine, carrier: &Carrier) {
        for freq_hz in self.repeaters_of(carrier) {
            for slot in carrier.spec.timeslots() {
                self.ensure_follower(
                    engine,
                    carrier,
                    FollowerKey::KnownCarrier {
                        system_node: carrier.system_node.clone(),
                        freq_hz,
                        slot,
                    },
                    Grant {
                        slot,
                        freq_hz,
                        call: None,
                    },
                );
            }
        }
    }

    pub(super) fn repeaters_of(&self, carrier: &Carrier) -> Vec<u64> {
        let mut freqs = vec![carrier.freq_hz];
        let entries = self
            .systems
            .iter()
            .find(|system| system.node == carrier.system_node)
            .map(|system| system.channel_map.iter().map(|entry| entry.freq_hz))
            .into_iter()
            .flatten();
        let found = self
            .prospectors
            .get(&carrier.system_node)
            .map(Prospector::repeaters)
            .unwrap_or_default();
        for freq_hz in entries.chain(found) {
            if !freqs.contains(&freq_hz) {
                freqs.push(freq_hz);
            }
        }
        freqs
    }

    pub(super) fn ensure_follower(
        &mut self,
        engine: &Engine,
        carrier: &Carrier,
        key: FollowerKey,
        grant: Grant,
    ) {
        if let Some(follower) = self.followers.get_mut(&key)
            && follower.freq_hz == grant.freq_hz
        {
            follower.granted = Instant::now();
            if grant.call.is_some() && follower.call != grant.call {
                follower.call = grant.call;
                self.publish();
            }
            return;
        }
        if self.backing_off(&key, grant.freq_hz) {
            return;
        }
        let params = match (carrier.trunking.follow)(
            grant.slot,
            carrier.ignore_crc,
            &carrier.keys,
            carrier.vocoder.as_deref(),
        ) {
            Ok(params) => params,
            Err(error) => {
                self.fail(key, grant, error.to_string());
                return;
            }
        };
        let settings = ChannelSettings {
            frequency_hz: grant.freq_hz as f64,
            squelch: sdrmm_wire::Squelch::Off,
            blanker: Default::default(),
            if_filter: Default::default(),
            params,
        };
        if !engine.hears(carrier.device_set, carrier.stream, &settings) {
            let device_set = carrier.device_set;
            let announce = self.fail(
                key,
                grant,
                format!(
                    "the radio is not tuned where it could hear {:.4} MHz",
                    grant.freq_hz as f64 / 1e6
                ),
            );
            if announce {
                engine.emit_scope(StateScope::DeviceSet(device_set));
            }
            return;
        }
        if !self.followers.contains_key(&key)
            && self.at_limit(key.system_node())
            && !self.make_room(&key, grant)
        {
            self.fail(
                key,
                grant,
                format!("the system already holds {MAX_FOLLOWERS_PER_SYSTEM} traffic channels"),
            );
            return;
        }
        let result = match self.followers.get(&key) {
            Some(follower) => engine
                .patch_channel(carrier.device_set, follower.channel, settings)
                .map(|()| follower.channel),
            None => engine.add_channel_for(
                carrier.device_set,
                carrier.stream,
                settings,
                Some(key.system_node()),
            ),
        };
        match result {
            Ok(channel) => {
                let recovered = self.problems.remove(&key).is_some();
                self.followers.insert(
                    key,
                    FollowerChannel {
                        device_set: carrier.device_set,
                        channel,
                        freq_hz: grant.freq_hz,
                        call: grant.call,
                        granted: Instant::now(),
                    },
                );
                self.publish();
                if recovered {
                    engine.emit_scope(StateScope::DeviceSet(carrier.device_set));
                }
            }
            Err(error) => {
                let device_set = carrier.device_set;
                let announce = self.fail(key, grant, error.to_string());
                if announce {
                    engine.emit_scope(StateScope::DeviceSet(device_set));
                }
            }
        }
    }

    /// Hands `key` the receiver of a granted channel no call needs any more, or, for a priority
    /// talkgroup, the one held longest by a call that is not.
    pub(super) fn make_room(&mut self, key: &FollowerKey, grant: Grant) -> bool {
        let now = Instant::now();
        let node = key.system_node();
        let talkgroups = self.talkgroups_of(node).cloned().unwrap_or_default();
        let priority = |call: Option<TrunkGrant>| {
            call.is_some_and(|call| {
                call.group_call != Some(false) && talkgroups.priority(call.destination)
            })
        };
        let urgent = priority(grant.call);
        let victim = self
            .followers
            .iter()
            .filter(|(candidate, _)| {
                candidate.system_node() == node && matches!(candidate, FollowerKey::Granted { .. })
            })
            .filter(|(_, follower)| follower.idle(now) || (urgent && !priority(follower.call)))
            .min_by_key(|(_, follower)| (!follower.idle(now), follower.granted))
            .map(|(candidate, _)| candidate.clone());
        let Some(victim) = victim else {
            return false;
        };
        let Some(follower) = self.followers.remove(&victim) else {
            return false;
        };
        self.followers.insert(key.clone(), follower);
        true
    }

    pub(super) fn at_limit(&self, system_node: &str) -> bool {
        self.followers
            .keys()
            .filter(|candidate| candidate.system_node() == system_node)
            .count()
            >= MAX_FOLLOWERS_PER_SYSTEM
    }

    pub(super) fn backing_off(&self, key: &FollowerKey, freq_hz: u64) -> bool {
        self.problems.get(key).is_some_and(|problem| {
            problem.freq_hz == freq_hz && Instant::now() < problem.next_attempt
        })
    }

    pub(super) fn fail(&mut self, key: FollowerKey, grant: Grant, reason: String) -> bool {
        let now = Instant::now();
        let existing = self
            .problems
            .get(&key)
            .filter(|problem| problem.freq_hz == grant.freq_hz);
        let attempts = existing.map_or(1, |problem| problem.attempts.saturating_add(1));
        let fresh = existing.is_none() || existing.is_some_and(|problem| problem.reason != reason);
        let since = existing.map_or_else(
            || format!("{:.9}", jiff::Timestamp::now()),
            |problem| problem.since.clone(),
        );
        let backoff = RETRY_BASE
            .saturating_mul(1u32 << attempts.saturating_sub(1).min(6))
            .min(RETRY_MAX);
        if fresh {
            tracing::warn!(
                node = key.system_node(),
                logical_channel = key.logical_channel(),
                slot = grant.slot,
                freq_hz = grant.freq_hz,
                reason,
                "cannot follow a trunked traffic channel"
            );
        } else {
            tracing::debug!(
                node = key.system_node(),
                freq_hz = grant.freq_hz,
                attempts,
                "trunked traffic channel still refused"
            );
        }
        self.problems.insert(
            key,
            Problem {
                freq_hz: grant.freq_hz,
                reason,
                since,
                attempts,
                next_attempt: now + backoff,
            },
        );
        if fresh {
            self.publish();
        }
        fresh
    }
}

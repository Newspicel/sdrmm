use std::time::Instant;

use sdrmm_wire::{ChannelParams, ChannelSettings, TrunkKind};

use super::{CONTROL_SILENCE, ControlChannel, Follower, TrunkRadio, prospect::Prospector};
use crate::Engine;

impl Follower {
    pub(super) fn tune_control_channels(&mut self, engine: &Engine) {
        let nodes: Vec<String> = self
            .systems
            .iter()
            .filter(|system| system.radio.is_some())
            .map(|system| system.node.clone())
            .collect();
        let dropped: Vec<(String, u32, u32)> = self
            .controls
            .iter()
            .filter(|(node, _)| !nodes.contains(node))
            .map(|(node, control)| (node.clone(), control.device_set, control.channel))
            .collect();
        for (node, device_set, channel) in dropped {
            self.controls.remove(&node);
            match engine.remove_channel(device_set, channel) {
                Ok(()) | Err(_) => {}
            }
        }
        let wanted: Vec<(String, TrunkRadio, String)> = self
            .systems
            .iter()
            .filter_map(|system| Some((system.node.clone(), system.radio?, system.decoder.clone())))
            .collect();
        for (node, radio, decoder) in wanted {
            if self
                .controls
                .get(&node)
                .is_some_and(|control| control.configured_hz == radio.control_hz)
            {
                continue;
            }
            let params = match control_params(&decoder) {
                Ok(params) => params,
                Err(error) => {
                    tracing::warn!(node, %error, "no decoder can run this trunk system");
                    continue;
                }
            };
            let settings = ChannelSettings {
                frequency_hz: radio.control_hz as f64,
                squelch: sdrmm_wire::Squelch::Off,
                blanker: Default::default(),
                if_filter: Default::default(),
                params,
            };
            let result = match self.controls.get(&node) {
                Some(control) => engine
                    .patch_channel(radio.device_set, control.channel, settings)
                    .map(|()| control.channel),
                None => {
                    engine.add_channel_for(radio.device_set, radio.stream, settings, Some(&node))
                }
            };
            match result {
                Ok(channel) => {
                    self.controls.insert(
                        node,
                        ControlChannel {
                            device_set: radio.device_set,
                            channel,
                            freq_hz: radio.control_hz,
                            configured_hz: radio.control_hz,
                            heard: Instant::now(),
                        },
                    );
                }
                Err(error) => tracing::warn!(
                    node,
                    freq_hz = radio.control_hz,
                    %error,
                    "could not open the trunk control channel"
                ),
            }
        }
    }

    /// Moves a control channel that has gone quiet to the next one the site is known to run,
    /// back round to the operator's own once the others have had their turn.
    pub(super) fn fail_over_silent_controls(&mut self) {
        let now = Instant::now();
        let silent: Vec<String> = self
            .controls
            .iter()
            .filter(|(_, control)| now.saturating_duration_since(control.heard) >= CONTROL_SILENCE)
            .map(|(node, _)| node.clone())
            .collect();
        for node in silent {
            let grants = self
                .carriers
                .values()
                .find(|carrier| carrier.system_node == node)
                .is_some_and(|carrier| self.kind_of(carrier) == Some(TrunkKind::Grants));
            let others = self
                .prospectors
                .get(&node)
                .map(Prospector::other_control_hz)
                .unwrap_or_default();
            let Some(control) = self.controls.get_mut(&node) else {
                continue;
            };
            control.heard = now;
            if !grants {
                continue;
            }
            let Some(next) = next_control(control.configured_hz, control.freq_hz, &others) else {
                continue;
            };
            tracing::info!(
                node,
                from_hz = control.freq_hz,
                to_hz = next,
                "control channel went quiet, trying another of the site's"
            );
            control.freq_hz = next;
        }
    }

    pub(super) fn retune_drifted(&mut self, engine: &Engine) {
        let state = engine.snapshot();
        let receivers: Vec<(u32, u32, u64)> = self
            .controls
            .values()
            .map(|control| (control.device_set, control.channel, control.freq_hz))
            .chain(
                self.followers
                    .values()
                    .map(|follower| (follower.device_set, follower.channel, follower.freq_hz)),
            )
            .chain(
                self.probes
                    .iter()
                    .map(|((device_set, channel), probe)| (*device_set, *channel, probe.freq_hz)),
            )
            .collect();
        for (device_set, channel, freq_hz) in receivers {
            let Some(set) = state.device_sets.iter().find(|set| set.id == device_set) else {
                continue;
            };
            let Some(info) = set.channels.iter().find(|info| info.id == channel) else {
                continue;
            };
            let frequency_hz = freq_hz as f64;
            if (info.settings.frequency_hz - frequency_hz).abs() < 1.0 {
                continue;
            }
            let settings = ChannelSettings {
                frequency_hz,
                ..info.settings.clone()
            };
            match engine.patch_channel(device_set, channel, settings) {
                Ok(()) => {
                    self.unreachable.remove(&(device_set, channel));
                }
                Err(error) => {
                    if self.unreachable.insert((device_set, channel)) {
                        tracing::warn!(
                            device_set,
                            channel,
                            freq_hz,
                            %error,
                            "could not follow the radio with a trunk receiver"
                        );
                    }
                }
            }
        }
    }
}

/// The control channel after `current_hz` in the ring of the operator's own and the site's others.
pub(super) fn next_control(configured_hz: u64, current_hz: u64, others: &[u64]) -> Option<u64> {
    let mut ring = vec![configured_hz];
    ring.extend(others.iter().copied().filter(|hz| *hz != configured_hz));
    if ring.len() < 2 {
        return None;
    }
    let at = ring.iter().position(|hz| *hz == current_hz).unwrap_or(0);
    ring.get((at + 1) % ring.len()).copied()
}

/// The control channel settings of the decoder a trunk system runs on.
fn control_params(decoder: &str) -> Result<ChannelParams, String> {
    let (_, trunking) = sdrmm_channels::trunking(decoder)
        .ok_or_else(|| format!("no registered decoder trunks as {decoder:?}"))?;
    (trunking.watch)(true).map_err(|error| error.to_string())
}

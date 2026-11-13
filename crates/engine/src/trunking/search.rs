use std::{sync::Arc, time::Instant};

use sdrmm_wire::{ChannelSettings, DvFrame};

use super::{Carrier, Follower, FollowerKey, Grant, Heard, Probe, carriers};
use crate::Engine;

impl Follower {
    pub(super) fn prospect(
        &mut self,
        engine: &Engine,
        system_node: &str,
        freq_hz: u64,
        frame: &DvFrame,
    ) -> bool {
        let Some(prospector) = self.prospectors.get_mut(system_node) else {
            return false;
        };
        if prospector.shares_traffic() {
            if !prospector.note_repeater(freq_hz, frame) {
                return false;
            }
            tracing::info!(
                node = system_node,
                freq_hz,
                "learned another repeater of a trunked system"
            );
            self.retire_probe(engine, system_node, freq_hz);
            self.publish();
            return true;
        }
        let Some(found) = prospector.note_probe(freq_hz, frame, Instant::now()) else {
            return false;
        };
        if !found.confirmed {
            self.publish();
            return false;
        }
        tracing::info!(
            node = system_node,
            logical_channel = found.logical_channel,
            freq_hz = found.freq_hz,
            "learned where a trunked logical channel transmits"
        );
        self.retire_probe(engine, system_node, found.freq_hz);
        let carrier = self
            .carriers
            .values()
            .find(|carrier| carrier.system_node == system_node)
            .cloned();
        if let Some(carrier) = carrier {
            let slot = found
                .slot
                .or_else(|| carrier.spec.timeslots().first().copied().flatten());
            self.ensure_follower(
                engine,
                &carrier,
                FollowerKey::Granted {
                    system_node: system_node.to_owned(),
                    logical_channel: found.logical_channel,
                    slot,
                },
                Grant {
                    slot,
                    freq_hz: found.freq_hz,
                    call: None,
                },
            );
        }
        self.publish();
        false
    }

    pub(super) fn retire_probe(&mut self, engine: &Engine, system_node: &str, freq_hz: u64) {
        let retired: Vec<(u32, u32)> = self
            .probes
            .iter()
            .filter(|(_, probe)| probe.system_node == system_node && probe.freq_hz == freq_hz)
            .map(|(key, _)| *key)
            .collect();
        for key in retired {
            self.close_probe(engine, key);
        }
    }

    pub(super) fn close_probe(&mut self, engine: &Engine, key: (u32, u32)) {
        self.probes.remove(&key);
        match engine.remove_channel(key.0, key.1) {
            Ok(()) => {}
            Err(error) if error.is_not_found() => {}
            Err(error) => tracing::warn!(
                device_set = key.0,
                channel = key.1,
                %error,
                "could not close a trunk search receiver"
            ),
        }
    }

    pub(super) fn note_carriers(&mut self, device_set: u32, heard: &[Heard]) -> bool {
        let now = Instant::now();
        let listening: Vec<String> = self
            .carriers
            .values()
            .filter(|carrier| carrier.device_set == device_set)
            .map(|carrier| carrier.system_node.clone())
            .collect();
        let mut woke = false;
        for node in listening {
            if let Some(prospector) = self.prospectors.get_mut(&node) {
                woke |= prospector.note_carriers(heard, now);
            }
        }
        woke
    }

    pub(super) fn watch_bands(&mut self, engine: &Arc<Engine>, live: &[Carrier]) {
        let wanted: Vec<(u32, u32)> = self
            .systems
            .iter()
            .filter(|system| system.discovery.enabled && system.discovery.valid())
            .filter_map(|system| {
                let carrier = live
                    .iter()
                    .find(|carrier| carrier.system_node == system.node)?;
                Some((carrier.device_set, carrier.stream))
            })
            .collect();
        self.watching
            .retain(|watch| watch.live() && wanted.contains(&(watch.device_set, watch.stream)));
        for (device_set, stream) in wanted {
            if self
                .watching
                .iter()
                .any(|watch| watch.device_set == device_set && watch.stream == stream)
            {
                continue;
            }
            if let Some(watch) = carriers::Watch::start(engine, self.tx.clone(), device_set, stream)
            {
                self.watching.push(watch);
            }
        }
    }

    pub(super) fn search(&mut self, engine: &Engine, live: &[Carrier]) {
        let now = Instant::now();
        let mut wanted: Vec<(String, u64, Carrier)> = Vec::new();
        let mut keep: Vec<(String, u64)> = Vec::new();
        for system in &self.systems {
            let Some(carrier) = live
                .iter()
                .find(|carrier| carrier.system_node == system.node)
            else {
                continue;
            };
            let Some(scheme) = self.scheme_of(carrier).cloned() else {
                continue;
            };
            let Some(prospector) = self.prospectors.get_mut(&system.node) else {
                continue;
            };
            prospector.set_scheme(Some(scheme));
            let Some(reach) = probe_reach(carrier) else {
                continue;
            };
            prospector.cover(carrier.freq_hz, &reach);
            let taken: Vec<u64> = self
                .followers
                .iter()
                .filter(|(key, _)| key.system_node() == system.node)
                .map(|(_, follower)| follower.freq_hz)
                .chain(std::iter::once(carrier.freq_hz))
                .collect();
            let open: Vec<u64> = self
                .probes
                .values()
                .filter(|probe| probe.system_node == system.node)
                .map(|probe| probe.freq_hz)
                .collect();
            for freq_hz in prospector.schedule(now, &open, |freq_hz| {
                !taken.contains(&freq_hz) && reach.contains(&freq_hz)
            }) {
                keep.push((system.node.clone(), freq_hz));
                if !open.contains(&freq_hz) {
                    wanted.push((system.node.clone(), freq_hz, carrier.clone()));
                }
            }
        }
        let stale: Vec<(u32, u32)> = self
            .probes
            .iter()
            .filter(|(_, probe)| {
                !keep
                    .iter()
                    .any(|(node, freq_hz)| *node == probe.system_node && *freq_hz == probe.freq_hz)
            })
            .map(|(key, _)| *key)
            .collect();
        for key in stale {
            self.close_probe(engine, key);
        }
        for (system_node, freq_hz, carrier) in wanted {
            self.open_probe(engine, &system_node, freq_hz, &carrier);
        }
    }

    pub(super) fn open_probe(
        &mut self,
        engine: &Engine,
        system_node: &str,
        freq_hz: u64,
        carrier: &Carrier,
    ) {
        let params = match (carrier.trunking.watch)(true) {
            Ok(params) => params,
            Err(error) => {
                tracing::warn!(system_node, %error, "could not build a trunk probe");
                return;
            }
        };
        let settings = ChannelSettings {
            frequency_hz: freq_hz as f64,
            squelch: sdrmm_wire::Squelch::Off,
            blanker: Default::default(),
            if_filter: Default::default(),
            params,
        };
        match engine.add_channel_for(
            carrier.device_set,
            carrier.stream,
            settings,
            Some(system_node),
        ) {
            Ok(channel) => {
                self.probes.insert(
                    (carrier.device_set, channel),
                    Probe {
                        system_node: system_node.to_owned(),
                        freq_hz,
                    },
                );
            }
            Err(error) => tracing::debug!(
                node = system_node,
                freq_hz,
                %error,
                "could not open a trunk search receiver"
            ),
        }
    }
}

fn probe_reach(carrier: &Carrier) -> Option<std::ops::RangeInclusive<u64>> {
    let Ok(watch) = (carrier.trunking.watch)(true) else {
        return None;
    };
    let (low, high) = sdrmm_channels::occupied_band(&watch);
    let nyquist = carrier.sample_rate / 2.0;
    let lowest = (carrier.center_hz - nyquist - low).max(0.0);
    let highest = carrier.center_hz + nyquist - high;
    if !lowest.is_finite() || !highest.is_finite() || highest < lowest {
        return None;
    }
    Some((lowest.ceil() as u64)..=(highest.floor() as u64))
}

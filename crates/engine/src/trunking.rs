mod carriers;
mod prospect;

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use prospect::Prospector;
use sdrmm_channels::Trunking;
use sdrmm_wire::{
    AUTO_TRUNK_PROTOCOL, ChannelParams, ChannelSettings, DecodedRecord, DvFrame, DvFrameKind,
    StateScope, TrunkChannel, TrunkChannelEntry, TrunkChannelSource, TrunkControl, TrunkDiscovery,
    TrunkFollower, TrunkKind, TrunkProbe, TrunkProblem, TrunkScheme, TrunkSite, TrunkSpec,
    TrunkSystemStatus, TrunkTalkgroups, VoiceKey,
};

use crate::Engine;

const RECONCILE_INTERVAL: Duration = Duration::from_secs(1);

const MAX_FOLLOWERS_PER_SYSTEM: usize = 16;

const RETRY_BASE: Duration = Duration::from_secs(5);
const RETRY_MAX: Duration = Duration::from_secs(300);

/// How long a granted traffic channel stays its call's after the last grant naming it, before
/// another call may take its receiver.
const GRANT_HOLD: Duration = Duration::from_secs(3);

/// How long a control channel may go without a control burst before the follower tries the
/// site's other control channels.
const CONTROL_SILENCE: Duration = Duration::from_secs(10);

/// Decoded frames queued for the trunk follower before new ones are dropped.
pub(crate) const INPUT_DEPTH: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrunkSystem {
    pub node: String,
    /// The decoder the node asked for; empty takes the first one that trunks.
    pub decoder: String,
    /// A protocol id the decoder declares, or [`AUTO_TRUNK_PROTOCOL`].
    pub protocol: String,
    pub discovery: TrunkDiscovery,
    pub channel_map: Vec<TrunkChannelEntry>,
    pub learned: Vec<TrunkChannelEntry>,
    pub radio: Option<TrunkRadio>,
    pub talkgroups: TrunkTalkgroups,
    pub keys: Vec<VoiceKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrunkRadio {
    pub device_set: u32,
    pub stream: u32,
    pub control_hz: u64,
    pub ignore_crc: bool,
}

pub(crate) enum TrunkInput {
    Record(Box<DecodedRecord>),
    Configure(Vec<TrunkSystem>),
    Carriers { device_set: u32, heard: Vec<Heard> },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Heard {
    pub freq_hz: u64,
    pub level_db: f32,
}

#[derive(Clone)]
struct Carrier {
    system_node: String,
    trunking: Trunking,
    spec: TrunkSpec,
    protocol: String,
    device_set: u32,
    stream: u32,
    center_hz: f64,
    sample_rate: f64,
    freq_hz: u64,
    ignore_crc: bool,
    keys: Vec<VoiceKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum FollowerKey {
    KnownCarrier {
        system_node: String,
        freq_hz: u64,
        slot: u8,
    },
    Granted {
        system_node: String,
        logical_channel: u16,
        slot: u8,
    },
}

impl FollowerKey {
    fn system_node(&self) -> &str {
        match self {
            Self::KnownCarrier { system_node, .. } | Self::Granted { system_node, .. } => {
                system_node
            }
        }
    }

    fn slot(&self) -> u8 {
        match self {
            Self::KnownCarrier { slot, .. } | Self::Granted { slot, .. } => *slot,
        }
    }

    fn logical_channel(&self) -> Option<u16> {
        match self {
            Self::KnownCarrier { .. } => None,
            Self::Granted {
                logical_channel, ..
            } => Some(*logical_channel),
        }
    }
}

struct FollowerChannel {
    device_set: u32,
    channel: u32,
    freq_hz: u64,
    call: Option<GrantedCall>,
    granted: Instant,
}

impl FollowerChannel {
    fn idle(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.granted) >= GRANT_HOLD
    }
}

/// The control channel a system listens to, and when it last heard the site on it.
struct ControlChannel {
    device_set: u32,
    channel: u32,
    freq_hz: u64,
    configured_hz: u64,
    heard: Instant,
}

struct Problem {
    freq_hz: u64,
    reason: String,
    since: String,
    attempts: u32,
    next_attempt: Instant,
}

#[derive(Clone, Copy)]
struct Grant {
    slot: u8,
    freq_hz: u64,
    call: Option<GrantedCall>,
}

pub(crate) fn spawn(
    engine: &Arc<Engine>,
    tx: mpsc::SyncSender<TrunkInput>,
    rx: mpsc::Receiver<TrunkInput>,
    status: Arc<Mutex<Vec<TrunkSystemStatus>>>,
    dropped: Arc<AtomicU64>,
) {
    let follower = Follower {
        engine: Arc::downgrade(engine),
        status,
        dropped,
        sites: HashMap::new(),

        systems: Vec::new(),
        carriers: HashMap::new(),
        definitions: HashMap::new(),
        followers: HashMap::new(),
        problems: HashMap::new(),
        detected: HashMap::new(),
        prospectors: HashMap::new(),
        probes: HashMap::new(),
        controls: HashMap::new(),
        unreachable: HashSet::new(),
        watching: Vec::new(),
        tx,
    };
    let spawned = std::thread::Builder::new()
        .name("sdrmm-trunk".to_string())
        .spawn(move || follower.run(&rx));
    if let Err(e) = spawned {
        tracing::error!("failed to spawn the trunk follower: {e}");
    }
}

struct Probe {
    system_node: String,
    freq_hz: u64,
}

struct Follower {
    engine: Weak<Engine>,
    status: Arc<Mutex<Vec<TrunkSystemStatus>>>,
    dropped: Arc<AtomicU64>,
    systems: Vec<TrunkSystem>,
    carriers: HashMap<(u32, u32), Carrier>,
    definitions: HashMap<(String, u16), u64>,
    followers: HashMap<FollowerKey, FollowerChannel>,
    problems: HashMap<FollowerKey, Problem>,
    detected: HashMap<String, String>,
    sites: HashMap<String, TrunkSite>,
    prospectors: HashMap<String, Prospector>,
    probes: HashMap<(u32, u32), Probe>,
    controls: HashMap<String, ControlChannel>,
    unreachable: HashSet<(u32, u32)>,
    watching: Vec<carriers::Watch>,
    tx: mpsc::SyncSender<TrunkInput>,
}

impl Follower {
    fn run(mut self, rx: &mpsc::Receiver<TrunkInput>) {
        let mut next_reconcile = Instant::now();
        loop {
            let wait = next_reconcile.saturating_duration_since(Instant::now());
            let woke = match rx.recv_timeout(wait) {
                Ok(TrunkInput::Record(record)) => self.observe(&record),
                Ok(TrunkInput::Carriers { device_set, heard }) => {
                    self.note_carriers(device_set, &heard)
                }
                Ok(TrunkInput::Configure(systems)) => {
                    self.systems = systems;
                    true
                }
                Err(mpsc::RecvTimeoutError::Timeout) => true,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            };
            if woke {
                self.reconcile();
                next_reconcile = Instant::now() + RECONCILE_INTERVAL;
            }
        }
    }

    fn resolve_carriers(&self, engine: &Engine) -> HashMap<(u32, u32), Carrier> {
        let state = engine.snapshot();
        let mut carriers = HashMap::new();
        for system in &self.systems {
            let Some(control) = self.controls.get(&system.node) else {
                continue;
            };
            let Some((decoder, trunking)) = sdrmm_channels::trunking(&system.decoder) else {
                continue;
            };
            let (device_set, channel) = (control.device_set, control.channel);
            let Some(set) = state.device_sets.iter().find(|set| set.id == device_set) else {
                continue;
            };
            let Some(info) = set.channels.iter().find(|info| info.id == channel) else {
                continue;
            };
            if info.settings.params.type_id() != decoder {
                continue;
            }
            let Some(center_hz) = set.settings.center_hz else {
                continue;
            };
            let absolute = info.settings.frequency_hz;
            if !absolute.is_finite() || absolute <= 0.0 {
                continue;
            }
            carriers.insert(
                (device_set, channel),
                Carrier {
                    system_node: system.node.clone(),
                    trunking,
                    spec: (trunking.spec)(),
                    protocol: system.protocol.clone(),
                    device_set,
                    stream: info.stream,
                    center_hz,
                    sample_rate: crate::sample_rate_of(&set.settings),
                    freq_hz: absolute.round() as u64,
                    ignore_crc: system.radio.is_some_and(|radio| radio.ignore_crc),
                    keys: system.keys.clone(),
                },
            );
        }
        carriers
    }

    fn scheme_of<'a>(&'a self, carrier: &'a Carrier) -> Option<&'a TrunkScheme> {
        let id = if carrier.protocol == AUTO_TRUNK_PROTOCOL {
            self.detected.get(&carrier.system_node)?
        } else {
            &carrier.protocol
        };
        carrier.spec.scheme(id)
    }

    fn kind_of(&self, carrier: &Carrier) -> Option<TrunkKind> {
        self.scheme_of(carrier).map(|scheme| scheme.kind)
    }

    fn talkgroups_of(&self, system_node: &str) -> Option<&TrunkTalkgroups> {
        self.systems
            .iter()
            .find(|system| system.node == system_node)
            .map(|system| &system.talkgroups)
    }

    fn observe(&mut self, record: &DecodedRecord) -> bool {
        let Some(engine) = self.engine.upgrade() else {
            return false;
        };
        let Some(frame) = record.event.read::<DvFrame>() else {
            return false;
        };
        let frame = &frame;
        if let Some(probe) = self.probes.get(&(record.device_set, record.channel)) {
            let (system_node, freq_hz) = (probe.system_node.clone(), probe.freq_hz);
            return self.prospect(&engine, &system_node, freq_hz, frame);
        }
        let Some(carrier) = self
            .carriers
            .get(&(record.device_set, record.channel))
            .cloned()
        else {
            return false;
        };
        if let Some(protocol) = &frame.trunk_protocol {
            self.note_detection(&carrier, protocol);
        }
        if frame.kind == DvFrameKind::Control {
            self.note_control(&carrier, frame);
        }
        match self.kind_of(&carrier) {
            Some(TrunkKind::Distributed) => {
                if let Some(prospector) = self.prospectors.get_mut(&carrier.system_node) {
                    prospector.note_rest(frame.rest_channel);
                }
                self.provision_known_carrier(&engine, &carrier);
                false
            }
            Some(TrunkKind::Grants) => self.observe_grants(&engine, &carrier, frame),
            None => false,
        }
    }

    fn note_detection(&mut self, carrier: &Carrier, protocol: &str) {
        let system_node = carrier.system_node.as_str();
        if self.detected.get(system_node).map(String::as_str) == Some(protocol)
            || carrier.spec.scheme(protocol).is_none()
        {
            return;
        }
        self.detected
            .insert(system_node.to_owned(), protocol.to_owned());
        tracing::info!(node = system_node, protocol, "trunk system identified");
        self.publish();
    }

    /// A control burst from the site: the control channel is alive, and it may name the site.
    fn note_control(&mut self, carrier: &Carrier, frame: &DvFrame) {
        if let Some(control) = self.controls.get_mut(&carrier.system_node) {
            control.heard = Instant::now();
        }
        let site = self.sites.entry(carrier.system_node.clone()).or_default();
        let before = *site;
        site.system_id = frame.system_id.or(site.system_id);
        site.network_id = frame.network_id.or(site.network_id);
        site.site_id = frame.site_id.or(site.site_id);
        if *site != before {
            self.publish();
        }
    }

    fn prospect(
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
            self.ensure_follower(
                engine,
                &carrier,
                FollowerKey::Granted {
                    system_node: system_node.to_owned(),
                    logical_channel: found.logical_channel,
                    slot: found.slot,
                },
                Grant {
                    slot: found.slot,
                    freq_hz: found.freq_hz,
                    call: None,
                },
            );
        }
        self.publish();
        false
    }

    fn retire_probe(&mut self, engine: &Engine, system_node: &str, freq_hz: u64) {
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

    fn close_probe(&mut self, engine: &Engine, key: (u32, u32)) {
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

    fn note_carriers(&mut self, device_set: u32, heard: &[Heard]) -> bool {
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

    fn watch_bands(&mut self, engine: &Arc<Engine>, live: &[Carrier]) {
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

    fn tune_control_channels(&mut self, engine: &Engine) {
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
    fn fail_over_silent_controls(&mut self) {
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

    fn retune_drifted(&mut self, engine: &Engine) {
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

    fn search(&mut self, engine: &Engine, live: &[Carrier]) {
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

    fn open_probe(&mut self, engine: &Engine, system_node: &str, freq_hz: u64, carrier: &Carrier) {
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

    fn observe_grants(&mut self, engine: &Engine, carrier: &Carrier, frame: &DvFrame) -> bool {
        if let Some(color_code) = frame.color_code
            && let Ok(color_code) = u8::try_from(color_code)
            && let Some(prospector) = self.prospectors.get_mut(&carrier.system_node)
        {
            prospector.note_site(color_code);
        }
        if let Some(definition) = &frame.channel_definition {
            self.definitions.insert(
                (carrier.system_node.clone(), definition.channel),
                definition.rx_hz,
            );
        }
        if frame.kind != DvFrameKind::Control {
            return false;
        }
        let mut woke = false;
        for call in granted_calls(frame) {
            woke |= self.follow_granted(engine, carrier, call);
        }
        woke
    }

    fn follow_granted(&mut self, engine: &Engine, carrier: &Carrier, call: GrantedCall) -> bool {
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

    fn provision_known_carrier(&mut self, engine: &Engine, carrier: &Carrier) {
        for freq_hz in self.repeaters_of(carrier) {
            for slot in 1..=carrier.spec.slots.max(1) {
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

    fn repeaters_of(&self, carrier: &Carrier) -> Vec<u64> {
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

    fn ensure_follower(
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
        let params = match (carrier.trunking.follow)(grant.slot, carrier.ignore_crc, &carrier.keys)
        {
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
    fn make_room(&mut self, key: &FollowerKey, grant: Grant) -> bool {
        let now = Instant::now();
        let node = key.system_node();
        let talkgroups = self.talkgroups_of(node).cloned().unwrap_or_default();
        let priority = |call: Option<GrantedCall>| {
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

    fn at_limit(&self, system_node: &str) -> bool {
        self.followers
            .keys()
            .filter(|candidate| candidate.system_node() == system_node)
            .count()
            >= MAX_FOLLOWERS_PER_SYSTEM
    }

    fn backing_off(&self, key: &FollowerKey, freq_hz: u64) -> bool {
        self.problems.get(key).is_some_and(|problem| {
            problem.freq_hz == freq_hz && Instant::now() < problem.next_attempt
        })
    }

    fn fail(&mut self, key: FollowerKey, grant: Grant, reason: String) -> bool {
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

    fn reconcile(&mut self) {
        let Some(engine) = self.engine.upgrade() else {
            return;
        };
        self.tune_control_channels(&engine);
        self.carriers = self.resolve_carriers(&engine);
        self.fail_over_silent_controls();
        self.retune_drifted(&engine);
        let live: Vec<Carrier> = self.carriers.values().cloned().collect();
        let known: Vec<(FollowerKey, u32, u32)> = self
            .followers
            .iter()
            .filter(|(key, _)| !self.system_holds(&live, key))
            .map(|(key, follower)| (key.clone(), follower.device_set, follower.channel))
            .collect();
        for (key, device_set, channel) in known {
            match engine.remove_channel(device_set, channel) {
                Ok(()) => {}
                Err(error) if error.is_not_found() => {}
                Err(error) => tracing::warn!(
                    device_set,
                    channel,
                    %error,
                    "could not remove an orphaned trunk follower"
                ),
            }
            self.followers.remove(&key);
        }
        let nodes: Vec<&str> = self
            .systems
            .iter()
            .map(|system| system.node.as_str())
            .collect();
        self.definitions
            .retain(|(node, _), _| nodes.contains(&node.as_str()));
        self.detected
            .retain(|node, _| nodes.contains(&node.as_str()));
        self.sites.retain(|node, _| nodes.contains(&node.as_str()));
        let settled: Vec<FollowerKey> = self
            .problems
            .keys()
            .filter(|key| !nodes.contains(&key.system_node()) || !self.system_holds(&live, key))
            .cloned()
            .collect();
        for key in settled {
            self.problems.remove(&key);
        }
        self.prospectors
            .retain(|node, _| nodes.contains(&node.as_str()));
        for system in &self.systems {
            match self.prospectors.get_mut(&system.node) {
                Some(prospector) => prospector.configure(&system.discovery, &system.channel_map),
                None => {
                    let mut prospector = Prospector::new(&system.discovery, &system.channel_map);
                    prospector.adopt(&system.learned);
                    self.prospectors.insert(system.node.clone(), prospector);
                }
            }
        }
        for carrier in &live {
            if self.kind_of(carrier) == Some(TrunkKind::Distributed) {
                self.provision_known_carrier(&engine, carrier);
            }
        }
        self.watch_bands(&engine, &live);
        let before = self.probes.len();
        self.search(&engine, &live);
        if self.probes.len() != before {
            for device_set in device_sets(&live) {
                engine.emit_scope(StateScope::DeviceSet(device_set));
            }
        }
        self.publish();
    }

    fn system_holds(&self, live: &[Carrier], key: &FollowerKey) -> bool {
        match key {
            FollowerKey::KnownCarrier {
                system_node,
                freq_hz,
                ..
            } => live.iter().any(|source| {
                source.system_node == *system_node
                    && self.kind_of(source) == Some(TrunkKind::Distributed)
                    && self.repeaters_of(source).contains(freq_hz)
            }),
            FollowerKey::Granted { system_node, .. } => live.iter().any(|source| {
                source.system_node == *system_node
                    && self.kind_of(source) == Some(TrunkKind::Grants)
            }),
        }
    }

    fn publish(&self) {
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

    fn status_of(&self, system: &TrunkSystem) -> TrunkSystemStatus {
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

    fn channel_map_of(&self, system_node: &str) -> Vec<TrunkChannel> {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GrantedCall {
    logical_channel: u16,
    slot: u8,
    destination: u32,
    source: Option<u32>,
    group_call: Option<bool>,
}

fn granted_calls(frame: &DvFrame) -> Vec<GrantedCall> {
    let updates: Vec<GrantedCall> = frame
        .slot_activity
        .iter()
        .filter_map(|activity| {
            Some(GrantedCall {
                logical_channel: activity.logical_channel?,
                slot: activity.slot,
                destination: activity.destination?,
                source: None,
                group_call: None,
            })
        })
        .collect();
    if !updates.is_empty() {
        return updates;
    }
    match (frame.channel, frame.slot, frame.destination) {
        (Some(logical_channel), Some(slot), Some(destination)) => vec![GrantedCall {
            logical_channel,
            slot,
            destination,
            source: frame.source,
            group_call: frame.group_call,
        }],
        _ => Vec::new(),
    }
}

/// The control channel after `current_hz` in the ring of the operator's own and the site's others.
fn next_control(configured_hz: u64, current_hz: u64, others: &[u64]) -> Option<u64> {
    let mut ring = vec![configured_hz];
    ring.extend(others.iter().copied().filter(|hz| *hz != configured_hz));
    if ring.len() < 2 {
        return None;
    }
    let at = ring.iter().position(|hz| *hz == current_hz).unwrap_or(0);
    ring.get((at + 1) % ring.len()).copied()
}

fn device_sets(live: &[Carrier]) -> Vec<u32> {
    let mut sets: Vec<u32> = live.iter().map(|carrier| carrier.device_set).collect();
    sets.sort_unstable();
    sets.dedup();
    sets
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

/// The control channel settings of the decoder a trunk system runs on.
fn control_params(decoder: &str) -> Result<ChannelParams, String> {
    let (_, trunking) = sdrmm_channels::trunking(decoder)
        .ok_or_else(|| format!("no registered decoder trunks as {decoder:?}"))?;
    (trunking.watch)(true).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use sdrmm_channels::{DmrParams, DmrSlots};
    use sdrmm_device::DeviceRegistry;
    use sdrmm_device_virtual::VirtualDriver;
    use sdrmm_wire::{DecoderEvent, DeviceSettings, DvChannelDefinition};

    use super::*;

    const LOGICAL_CHANNEL: u16 = 17;

    fn engine() -> Arc<Engine> {
        let mut registry = DeviceRegistry::new();
        registry.register(1, Box::new(VirtualDriver::new()));
        Engine::with_registry(registry, None)
    }

    fn control_channel(engine: &Engine) -> (u32, u32, f64) {
        let device_set = engine
            .create_device_set("virtual:band")
            .expect("virtual device");
        let center_hz = engine.snapshot().device_sets[0]
            .settings
            .center_hz
            .expect("a tuned radio");
        let channel = engine
            .add_channel(
                device_set,
                0,
                ChannelSettings {
                    frequency_hz: center_hz,
                    squelch: sdrmm_wire::Squelch::Off,
                    params: ChannelParams::new("dmr", &DmrParams::default()).expect("dmr settings"),
                    blanker: Default::default(),
                    if_filter: Default::default(),
                },
            )
            .expect("control channel");
        (device_set, channel, center_hz)
    }

    fn follower(engine: &Arc<Engine>, protocol: &str, carrier: (u32, u32)) -> Follower {
        searching_follower(
            engine,
            protocol,
            carrier,
            TrunkDiscovery::default(),
            Vec::new(),
        )
    }

    fn searching_follower(
        engine: &Arc<Engine>,
        protocol: &str,
        carrier: (u32, u32),
        discovery: TrunkDiscovery,
        channel_map: Vec<TrunkChannelEntry>,
    ) -> Follower {
        let control_hz = engine
            .snapshot()
            .device_sets
            .iter()
            .find(|set| set.id == carrier.0)
            .and_then(|set| set.settings.center_hz)
            .expect("a tuned radio") as u64;
        let mut follower = Follower {
            engine: Arc::downgrade(engine),
            status: Arc::new(Mutex::new(Vec::new())),
            dropped: Arc::new(AtomicU64::new(0)),
            sites: HashMap::new(),
            systems: vec![TrunkSystem {
                node: "trunk".to_owned(),
                decoder: String::new(),
                protocol: protocol.to_owned(),
                discovery,
                channel_map,
                learned: Vec::new(),
                talkgroups: TrunkTalkgroups::default(),
                keys: Vec::new(),
                radio: Some(TrunkRadio {
                    device_set: carrier.0,
                    stream: 0,
                    control_hz,
                    ignore_crc: false,
                }),
            }],
            controls: HashMap::from([(
                "trunk".to_owned(),
                ControlChannel {
                    device_set: carrier.0,
                    channel: carrier.1,
                    freq_hz: control_hz,
                    configured_hz: control_hz,
                    heard: Instant::now(),
                },
            )]),
            carriers: HashMap::new(),
            definitions: HashMap::new(),
            followers: HashMap::new(),
            problems: HashMap::new(),
            detected: HashMap::new(),
            prospectors: HashMap::new(),
            probes: HashMap::new(),
            unreachable: HashSet::new(),
            watching: Vec::new(),
            tx: mpsc::sync_channel(1).0,
        };
        follower.reconcile();
        follower
    }

    fn record(carrier: (u32, u32), frame: DvFrame) -> DecodedRecord {
        DecodedRecord {
            origin: None,
            sinks: Vec::new(),
            callsign: None,
            device_set: carrier.0,
            channel: carrier.1,
            at: "2026-08-14T10:00:00Z".to_owned(),
            freq_hz: 451_000_000.0,
            event: DecoderEvent::of(&frame),
            view: None,
        }
    }

    fn definition(rx_hz: u64) -> DvFrame {
        DvFrame {
            trunk_protocol: Some("tier_three".to_owned()),
            crc_verified: Some(true),
            channel_definition: Some(DvChannelDefinition {
                channel: LOGICAL_CHANNEL,
                tx_hz: rx_hz,
                rx_hz,
                color_code: None,
            }),
            ..DvFrame::new("DMR", DvFrameKind::Control)
        }
    }

    fn grant(slot: u8) -> DvFrame {
        DvFrame {
            trunk_protocol: Some("tier_three".to_owned()),
            crc_verified: Some(true),
            channel: Some(LOGICAL_CHANNEL),
            slot: Some(slot),
            destination: Some(91),
            ..DvFrame::new("DMR", DvFrameKind::Control)
        }
    }

    fn search_over(center_hz: f64) -> TrunkDiscovery {
        TrunkDiscovery {
            enabled: true,
            ranges: vec![sdrmm_wire::TrunkSearchRange {
                start_hz: center_hz as u64 + 12_500,
                end_hz: center_hz as u64 + 50_000,
                step_hz: 12_500,
            }],
            max_probes: 4,
        }
    }

    fn traffic(destination: u32, source: u32, slot: u8) -> DvFrame {
        DvFrame {
            slot: Some(slot),
            destination: Some(destination),
            source: Some(source),
            ..DvFrame::new("DMR", DvFrameKind::Header)
        }
    }

    fn identified_grant(slot: u8) -> DvFrame {
        DvFrame {
            source: Some(4001),
            ..grant(slot)
        }
    }

    fn status(follower: &Follower) -> TrunkSystemStatus {
        follower
            .status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .first()
            .cloned()
            .expect("one system")
    }

    #[test]
    fn a_grant_on_a_defined_channel_opens_a_slot_filtered_receiver() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = follower(&engine, "tier_three", carrier);
        let traffic = center_hz as u64 + 125_000;

        follower.observe(&record(carrier, definition(traffic)));
        follower.observe(&record(carrier, grant(2)));

        let set = engine.snapshot().device_sets.remove(0);
        assert_eq!(set.channels.len(), 2, "the follower was not created");
        let followed = &set.channels[1];
        assert_eq!(followed.node.as_deref(), Some("trunk"));
        assert_eq!(followed.settings.frequency_hz, traffic as f64);
        assert_eq!(
            followed.settings.params,
            ChannelParams::new(
                "dmr",
                &DmrParams {
                    slots: DmrSlots::Two,
                    ignore_crc: false,
                    ..DmrParams::default()
                }
            )
            .expect("dmr settings")
        );
        let status = status(&follower);
        assert_eq!(status.followers.len(), 1);
        assert_eq!(status.followers[0].freq_hz, traffic);
        assert_eq!(status.followers[0].logical_channel, Some(LOGICAL_CHANNEL));
        assert!(status.problems.is_empty());
    }

    #[test]
    fn a_followed_call_is_deciphered_with_the_systems_keys() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = follower(&engine, "tier_three", carrier);
        let keys = vec![VoiceKey {
            key_id: 137,
            key: "58FFA35A8C".to_owned(),
        }];
        follower.systems[0].keys = keys.clone();
        follower.reconcile();
        let traffic = center_hz as u64 + 125_000;

        follower.observe(&record(carrier, definition(traffic)));
        follower.observe(&record(carrier, grant(2)));

        let set = engine.snapshot().device_sets.remove(0);
        let params = set.channels[1]
            .settings
            .params
            .parse::<DmrParams>()
            .expect("dmr settings");
        assert_eq!(params.keys, keys);
    }

    #[test]
    fn a_key_the_decoder_refuses_is_reported_as_a_problem() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = follower(&engine, "tier_three", carrier);
        follower.systems[0].keys = vec![VoiceKey {
            key_id: 1,
            key: "123456".to_owned(),
        }];
        follower.reconcile();
        let traffic = center_hz as u64 + 125_000;

        follower.observe(&record(carrier, definition(traffic)));
        follower.observe(&record(carrier, grant(2)));

        assert_eq!(engine.snapshot().device_sets[0].channels.len(), 1);
        assert_eq!(status(&follower).problems.len(), 1);
    }

    #[test]
    fn a_grant_for_an_undefined_channel_opens_nothing() {
        let engine = engine();
        let (device_set, channel, _) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = follower(&engine, "tier_three", carrier);

        follower.observe(&record(carrier, grant(1)));

        assert_eq!(engine.snapshot().device_sets[0].channels.len(), 1);
        assert!(follower.followers.is_empty());
    }

    #[test]
    fn a_site_that_checksums_its_own_way_is_still_followed() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = follower(&engine, "tier_three", carrier);
        let traffic = center_hz as u64 + 125_000;

        follower.observe(&record(carrier, definition(traffic)));
        let mut unverified = grant(1);
        unverified.crc_verified = Some(false);
        follower.observe(&record(carrier, unverified));

        let status = status(&follower);
        assert_eq!(status.followers.len(), 1, "the grant was thrown away");
        assert_eq!(status.followers[0].freq_hz, traffic);
    }

    #[test]
    fn auto_follows_capacity_plus_once_its_signalling_identifies_it() {
        let engine = engine();
        let (device_set, channel, _) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = follower(&engine, AUTO_TRUNK_PROTOCOL, carrier);
        assert!(
            follower.followers.is_empty(),
            "auto followed before it knew"
        );

        follower.observe(&record(
            carrier,
            DvFrame {
                trunk_protocol: Some("capacity_plus".to_owned()),
                crc_verified: Some(true),
                ..DvFrame::new("DMR", DvFrameKind::Control)
            },
        ));

        let set = engine.snapshot().device_sets.remove(0);
        assert_eq!(set.channels.len(), 3, "both slots were not provisioned");
        let status = status(&follower);
        assert_eq!(status.detected, Some("capacity_plus".to_owned()));
        assert_eq!(status.followers.len(), 2);
        assert_eq!(status.followers[0].slot, 1);
        assert_eq!(status.followers[1].slot, 2);
    }

    #[test]
    fn capacity_plus_keeps_both_slots_of_every_repeater_the_plan_names() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let second = center_hz as u64 + 25_000;
        let mut follower = searching_follower(
            &engine,
            "capacity_plus",
            carrier,
            TrunkDiscovery::default(),
            vec![TrunkChannelEntry {
                lcn: 2,
                freq_hz: second,
            }],
        );

        follower.observe(&record(
            carrier,
            DvFrame {
                trunk_protocol: Some("capacity_plus".to_owned()),
                crc_verified: Some(true),
                ..DvFrame::new("DMR", DvFrameKind::Control)
            },
        ));

        let planned = status(&follower);
        assert_eq!(planned.followers.len(), 4);
        assert!(
            planned
                .followers
                .iter()
                .filter(|follower| follower.freq_hz == second)
                .count()
                == 2,
            "the second repeater of the plan was never followed"
        );

        follower.systems[0].channel_map.clear();
        follower.reconcile();

        let after = status(&follower);
        assert_eq!(
            after.followers.len(),
            2,
            "a forgotten repeater kept running"
        );
        assert!(after.followers.iter().all(|kept| kept.freq_hz != second));
    }

    #[test]
    fn auto_follows_hytera_xpt_once_its_signalling_identifies_it() {
        let engine = engine();
        let (device_set, channel, _) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = follower(&engine, AUTO_TRUNK_PROTOCOL, carrier);
        assert!(
            follower.followers.is_empty(),
            "auto followed before it knew"
        );

        follower.observe(&record(
            carrier,
            DvFrame {
                trunk_protocol: Some("hytera_xpt".to_owned()),
                crc_verified: Some(true),
                ..DvFrame::new("DMR", DvFrameKind::Control)
            },
        ));

        let set = engine.snapshot().device_sets.remove(0);
        assert_eq!(set.channels.len(), 3, "both slots were not provisioned");
        assert!(set.channels.iter().any(|channel| {
            channel.settings.params.type_id == "dmr"
                && channel
                    .settings
                    .params
                    .parse::<DmrParams>()
                    .is_ok_and(|params| params.slots == DmrSlots::One)
        }));
        assert!(set.channels.iter().any(|channel| {
            channel.settings.params.type_id == "dmr"
                && channel
                    .settings
                    .params
                    .parse::<DmrParams>()
                    .is_ok_and(|params| params.slots == DmrSlots::Two)
        }));
        let status = status(&follower);
        assert_eq!(status.detected, Some("hytera_xpt".to_owned()));
        assert_eq!(status.followers.len(), 2);
        assert_eq!(status.followers[0].slot, 1);
        assert_eq!(status.followers[1].slot, 2);
    }

    #[test]
    fn a_grant_outside_the_sampled_band_is_reported_once_and_backed_off() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = follower(&engine, "tier_three", carrier);
        let far = center_hz as u64 + 10_000_000;

        follower.observe(&record(carrier, definition(far)));
        follower.observe(&record(carrier, grant(1)));
        follower.observe(&record(carrier, grant(1)));

        assert_eq!(engine.snapshot().device_sets[0].channels.len(), 1);
        let status = status(&follower);
        assert_eq!(status.problems.len(), 1);
        assert_eq!(status.problems[0].freq_hz, far);
        assert_eq!(status.problems[0].attempts, 1, "the refusal was retried");
        assert!(!status.problems[0].reason.is_empty());
    }

    #[test]
    fn dropping_the_system_closes_the_receivers_it_opened() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = follower(&engine, "tier_three", carrier);
        follower.observe(&record(carrier, definition(center_hz as u64 + 125_000)));
        follower.observe(&record(carrier, grant(1)));
        assert_eq!(engine.snapshot().device_sets[0].channels.len(), 2);

        follower.systems.clear();
        follower.reconcile();

        assert_eq!(
            engine.snapshot().device_sets[0].channels.len(),
            0,
            "the orphaned follower was left running"
        );
        assert!(follower.followers.is_empty());
        assert!(
            follower
                .status
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
    }

    #[test]
    fn a_system_given_a_radio_opens_its_own_control_channel() {
        let engine = engine();
        let device_set = engine
            .create_device_set("virtual:band")
            .expect("virtual device");
        let center_hz = engine.snapshot().device_sets[0]
            .settings
            .center_hz
            .expect("a tuned radio");
        let control_hz = center_hz as u64 + 12_500;
        let mut follower = Follower {
            engine: Arc::downgrade(&engine),
            status: Arc::new(Mutex::new(Vec::new())),
            dropped: Arc::new(AtomicU64::new(0)),
            sites: HashMap::new(),
            systems: vec![TrunkSystem {
                node: "trunk".to_owned(),
                decoder: String::new(),
                protocol: "tier_three".to_owned(),
                discovery: TrunkDiscovery::default(),
                channel_map: Vec::new(),
                learned: Vec::new(),
                talkgroups: TrunkTalkgroups::default(),
                keys: Vec::new(),
                radio: Some(TrunkRadio {
                    device_set,
                    stream: 0,
                    control_hz,
                    ignore_crc: false,
                }),
            }],
            carriers: HashMap::new(),
            definitions: HashMap::new(),
            followers: HashMap::new(),
            problems: HashMap::new(),
            detected: HashMap::new(),
            prospectors: HashMap::new(),
            probes: HashMap::new(),
            controls: HashMap::new(),
            unreachable: HashSet::new(),
            watching: Vec::new(),
            tx: mpsc::sync_channel(1).0,
        };

        follower.reconcile();

        let set = engine.snapshot().device_sets.remove(0);
        assert_eq!(set.channels.len(), 1, "no control receiver was opened");
        assert_eq!(set.channels[0].settings.frequency_hz, control_hz as f64);
        assert_eq!(status(&follower).carriers, 1);
        assert_eq!(
            status(&follower).control,
            Some(TrunkControl {
                device_set,
                channel: set.channels[0].id,
                freq_hz: control_hz,
            }),
            "the status did not name the control channel"
        );

        follower.systems.clear();
        follower.reconcile();

        assert!(
            engine.snapshot().device_sets[0].channels.is_empty(),
            "the control receiver outlived the system"
        );
    }

    #[test]
    fn moving_the_control_frequency_retunes_the_receiver_in_place() {
        let engine = engine();
        let device_set = engine
            .create_device_set("virtual:band")
            .expect("virtual device");
        let center_hz = engine.snapshot().device_sets[0]
            .settings
            .center_hz
            .expect("a tuned radio");
        let radio = |control_hz| {
            Some(TrunkRadio {
                device_set,
                stream: 0,
                control_hz,
                ignore_crc: false,
            })
        };
        let mut follower = Follower {
            engine: Arc::downgrade(&engine),
            status: Arc::new(Mutex::new(Vec::new())),
            dropped: Arc::new(AtomicU64::new(0)),
            sites: HashMap::new(),
            systems: vec![TrunkSystem {
                node: "trunk".to_owned(),
                decoder: String::new(),
                protocol: "tier_three".to_owned(),
                discovery: TrunkDiscovery::default(),
                channel_map: Vec::new(),
                learned: Vec::new(),
                talkgroups: TrunkTalkgroups::default(),
                keys: Vec::new(),
                radio: radio(center_hz as u64 + 12_500),
            }],
            carriers: HashMap::new(),
            definitions: HashMap::new(),
            followers: HashMap::new(),
            problems: HashMap::new(),
            detected: HashMap::new(),
            prospectors: HashMap::new(),
            probes: HashMap::new(),
            controls: HashMap::new(),
            unreachable: HashSet::new(),
            watching: Vec::new(),
            tx: mpsc::sync_channel(1).0,
        };
        follower.reconcile();
        let opened = engine.snapshot().device_sets[0].channels[0].id;

        follower.systems[0].radio = radio(center_hz as u64 + 25_000);
        follower.reconcile();

        let set = engine.snapshot().device_sets.remove(0);
        assert_eq!(set.channels.len(), 1, "a second control receiver appeared");
        assert_eq!(set.channels[0].id, opened, "the receiver was rebuilt");
        assert_eq!(set.channels[0].settings.frequency_hz, center_hz + 25_000.0);
    }

    #[test]
    fn retuning_the_radio_keeps_the_control_receiver_on_its_frequency() {
        let engine = engine();
        let device_set = engine
            .create_device_set("virtual:band")
            .expect("virtual device");
        let center_hz = engine.snapshot().device_sets[0]
            .settings
            .center_hz
            .expect("a tuned radio");
        let control_hz = center_hz as u64 + 12_500;
        let mut follower = Follower {
            engine: Arc::downgrade(&engine),
            status: Arc::new(Mutex::new(Vec::new())),
            dropped: Arc::new(AtomicU64::new(0)),
            sites: HashMap::new(),
            systems: vec![TrunkSystem {
                node: "trunk".to_owned(),
                decoder: String::new(),
                protocol: "tier_three".to_owned(),
                discovery: TrunkDiscovery::default(),
                channel_map: Vec::new(),
                learned: Vec::new(),
                talkgroups: TrunkTalkgroups::default(),
                keys: Vec::new(),
                radio: Some(TrunkRadio {
                    device_set,
                    stream: 0,
                    control_hz,
                    ignore_crc: false,
                }),
            }],
            carriers: HashMap::new(),
            definitions: HashMap::new(),
            followers: HashMap::new(),
            problems: HashMap::new(),
            detected: HashMap::new(),
            prospectors: HashMap::new(),
            probes: HashMap::new(),
            controls: HashMap::new(),
            unreachable: HashSet::new(),
            watching: Vec::new(),
            tx: mpsc::sync_channel(1).0,
        };
        follower.reconcile();
        let opened = engine.snapshot().device_sets[0].channels[0].id;

        engine
            .patch_device(
                device_set,
                DeviceSettings {
                    center_hz: Some(center_hz + 100_000.0),
                    ..DeviceSettings::default()
                },
            )
            .expect("retune the radio");
        follower.reconcile();

        let set = engine.snapshot().device_sets.remove(0);
        assert_eq!(set.channels.len(), 1, "a second control receiver appeared");
        assert_eq!(set.channels[0].id, opened, "the receiver was rebuilt");
        assert_eq!(
            set.channels[0].settings.frequency_hz, control_hz as f64,
            "the control receiver drifted with the radio"
        );
    }

    #[test]
    fn a_data_call_is_followed_like_any_other() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let traffic_hz = center_hz as u64 + 25_000;
        let mut follower = searching_follower(
            &engine,
            "tier_three",
            carrier,
            TrunkDiscovery::default(),
            vec![TrunkChannelEntry {
                lcn: LOGICAL_CHANNEL,
                freq_hz: traffic_hz,
            }],
        );

        follower.observe(&record(
            carrier,
            DvFrame {
                trunk_protocol: Some("tier_three".to_owned()),
                crc_verified: Some(true),
                channel: Some(LOGICAL_CHANNEL),
                slot: Some(1),
                destination: Some(9_999),
                source: Some(9_998),
                data: Some("data call".to_owned()),
                ..DvFrame::new("DMR", DvFrameKind::Control)
            },
        ));

        let status = status(&follower);
        assert_eq!(status.followers.len(), 1, "a data call went unfollowed");
        assert_eq!(status.followers[0].freq_hz, traffic_hz);
    }

    #[test]
    fn a_capacity_max_channel_update_follows_every_busy_timeslot() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let traffic_hz = center_hz as u64 + 25_000;
        let mut follower = searching_follower(
            &engine,
            "tier_three",
            carrier,
            TrunkDiscovery::default(),
            vec![TrunkChannelEntry {
                lcn: LOGICAL_CHANNEL,
                freq_hz: traffic_hz,
            }],
        );

        follower.observe(&record(
            carrier,
            DvFrame {
                trunk_protocol: Some("tier_three".to_owned()),
                crc_verified: Some(true),
                channel: Some(LOGICAL_CHANNEL),
                slot_activity: vec![
                    sdrmm_wire::DvSlotActivity {
                        slot: 1,
                        activity: "group voice".to_owned(),
                        destination_hash: None,
                        destination: Some(9_001),
                        logical_channel: Some(LOGICAL_CHANNEL),
                    },
                    sdrmm_wire::DvSlotActivity {
                        slot: 2,
                        activity: "group voice".to_owned(),
                        destination_hash: None,
                        destination: Some(9_002),
                        logical_channel: Some(LOGICAL_CHANNEL),
                    },
                ],
                ..DvFrame::new("DMR", DvFrameKind::Control)
            },
        ));

        let status = status(&follower);
        assert_eq!(status.followers.len(), 2, "one timeslot went unfollowed");
        assert!(
            status
                .followers
                .iter()
                .all(|open| open.freq_hz == traffic_hz)
        );
        assert_eq!(status.followers[0].slot, 1);
        assert_eq!(status.followers[1].slot, 2);
    }

    #[test]
    fn a_channel_update_for_an_unknown_channel_starts_the_search() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = searching_follower(
            &engine,
            "tier_three",
            carrier,
            search_over(center_hz),
            Vec::new(),
        );

        follower.observe(&record(
            carrier,
            DvFrame {
                trunk_protocol: Some("tier_three".to_owned()),
                crc_verified: Some(true),
                channel: Some(LOGICAL_CHANNEL),
                slot_activity: vec![sdrmm_wire::DvSlotActivity {
                    slot: 1,
                    activity: "group voice".to_owned(),
                    destination_hash: None,
                    destination: Some(9_001),
                    logical_channel: Some(LOGICAL_CHANNEL),
                }],
                ..DvFrame::new("DMR", DvFrameKind::Control)
            },
        ));
        follower.reconcile();

        assert_eq!(status(&follower).searching, 1);
        assert_eq!(status(&follower).probes.len(), 4);
    }

    #[test]
    fn a_search_with_no_band_named_reports_the_reach_it_settled_on() {
        let engine = engine();
        let (device_set, channel, _) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = searching_follower(
            &engine,
            "tier_three",
            carrier,
            TrunkDiscovery {
                enabled: true,
                ranges: Vec::new(),
                max_probes: 4,
            },
            Vec::new(),
        );

        follower.observe(&record(carrier, identified_grant(1)));
        follower.reconcile();

        let status = status(&follower);
        assert!(
            status.candidates > 1,
            "an unnamed search reported {} frequencies",
            status.candidates
        );
    }

    #[test]
    fn a_grant_for_an_unknown_channel_opens_search_receivers() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = searching_follower(
            &engine,
            "tier_three",
            carrier,
            search_over(center_hz),
            Vec::new(),
        );
        assert!(follower.probes.is_empty(), "searched before it was asked");

        follower.observe(&record(carrier, identified_grant(1)));
        follower.reconcile();

        let status = status(&follower);
        assert_eq!(status.searching, 1);
        assert_eq!(
            status.candidates, 4,
            "the status did not say how wide it looks"
        );
        assert_eq!(status.probes.len(), 4, "the raster was not swept");
        assert!(
            status
                .probes
                .iter()
                .all(|probe| probe.freq_hz > center_hz as u64),
            "a search receiver landed on the control channel"
        );
        let set = engine.snapshot().device_sets.remove(0);
        assert_eq!(set.channels.len(), 5);
        assert!(
            set.channels[1..]
                .iter()
                .all(|channel| channel.node.as_deref() == Some("trunk")),
            "a search receiver opened without its system node"
        );
    }

    #[test]
    fn the_control_channel_opens_for_the_system_node() {
        let engine = engine();
        let device_set = engine
            .create_device_set("virtual:band")
            .expect("virtual device");
        let mut follower = follower(&engine, "tier_three", (device_set, u32::MAX));
        follower.controls.clear();
        follower.reconcile();

        let set = engine.snapshot().device_sets.remove(0);
        assert_eq!(set.channels.len(), 1);
        assert_eq!(set.channels[0].node.as_deref(), Some("trunk"));
    }

    #[test]
    fn traffic_that_answers_a_grant_teaches_the_system_where_the_channel_lives() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = searching_follower(
            &engine,
            "tier_three",
            carrier,
            search_over(center_hz),
            Vec::new(),
        );
        follower.observe(&record(carrier, identified_grant(1)));
        follower.reconcile();
        let probe = status(&follower).probes[1];

        for _ in 0..2 {
            follower.observe(&record(
                (probe.device_set, probe.channel),
                traffic(91, 4001, 1),
            ));
        }

        let status = status(&follower);
        assert_eq!(status.channel_map.len(), 1);
        assert_eq!(status.channel_map[0].logical_channel, LOGICAL_CHANNEL);
        assert_eq!(status.channel_map[0].freq_hz, probe.freq_hz);
        assert_eq!(status.channel_map[0].source, TrunkChannelSource::Learned);
        assert_eq!(status.searching, 0);
        assert!(
            !status
                .probes
                .iter()
                .any(|open| open.freq_hz == probe.freq_hz),
            "the search receiver stayed open on a solved channel"
        );
        assert_eq!(status.followers.len(), 1, "the live call was not picked up");
        assert_eq!(status.followers[0].freq_hz, probe.freq_hz);
        assert_eq!(status.followers[0].logical_channel, Some(LOGICAL_CHANNEL));
    }

    #[test]
    fn a_learned_channel_is_followed_without_searching_again() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = searching_follower(
            &engine,
            "tier_three",
            carrier,
            search_over(center_hz),
            Vec::new(),
        );
        follower.observe(&record(carrier, identified_grant(1)));
        follower.reconcile();
        let probe = status(&follower).probes[1];
        for _ in 0..2 {
            follower.observe(&record(
                (probe.device_set, probe.channel),
                traffic(91, 4001, 1),
            ));
        }

        follower.observe(&record(carrier, identified_grant(2)));
        follower.reconcile();

        let status = status(&follower);
        assert_eq!(status.followers.len(), 2);
        assert!(
            status
                .followers
                .iter()
                .all(|open| open.freq_hz == probe.freq_hz)
        );
        assert!(status.probes.is_empty(), "it kept hunting a solved channel");
    }

    #[test]
    fn a_manual_channel_map_is_followed_without_any_search() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let traffic_hz = center_hz as u64 + 25_000;
        let mut follower = searching_follower(
            &engine,
            "tier_three",
            carrier,
            search_over(center_hz),
            vec![TrunkChannelEntry {
                lcn: LOGICAL_CHANNEL,
                freq_hz: traffic_hz,
            }],
        );

        follower.observe(&record(carrier, identified_grant(1)));
        follower.reconcile();

        let status = status(&follower);
        assert!(status.probes.is_empty(), "it searched for a known channel");
        assert_eq!(status.followers.len(), 1);
        assert_eq!(status.followers[0].freq_hz, traffic_hz);
        assert_eq!(status.channel_map[0].source, TrunkChannelSource::Manual);
    }

    #[test]
    fn an_announced_channel_never_starts_a_search() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = searching_follower(
            &engine,
            "tier_three",
            carrier,
            search_over(center_hz),
            Vec::new(),
        );

        follower.observe(&record(carrier, definition(center_hz as u64 + 12_500)));
        follower.observe(&record(carrier, identified_grant(1)));
        follower.reconcile();

        let status = status(&follower);
        assert!(status.probes.is_empty());
        assert_eq!(status.searching, 0);
        assert_eq!(status.channel_map.len(), 1);
        assert_eq!(status.channel_map[0].source, TrunkChannelSource::Announced);
    }

    #[test]
    fn dropping_the_system_closes_the_search_receivers_it_opened() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = searching_follower(
            &engine,
            "tier_three",
            carrier,
            search_over(center_hz),
            Vec::new(),
        );
        follower.observe(&record(carrier, identified_grant(1)));
        follower.reconcile();
        assert_eq!(engine.snapshot().device_sets[0].channels.len(), 5);

        follower.systems.clear();
        follower.reconcile();

        assert_eq!(
            engine.snapshot().device_sets[0].channels.len(),
            0,
            "orphaned search receivers were left running"
        );
        assert!(follower.probes.is_empty());
    }

    #[test]
    fn a_search_receiver_that_finds_a_control_channel_is_not_mistaken_for_traffic() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = searching_follower(
            &engine,
            "tier_three",
            carrier,
            search_over(center_hz),
            Vec::new(),
        );
        follower.observe(&record(carrier, identified_grant(1)));
        follower.reconcile();
        let probe = status(&follower).probes[0];

        for _ in 0..2 {
            let mut announced = identified_grant(1);
            announced.control_channel = Some(true);
            follower.observe(&record((probe.device_set, probe.channel), announced));
        }
        follower.reconcile();

        let status = status(&follower);
        assert!(status.channel_map.is_empty(), "a control channel was bound");
        assert!(
            !status
                .probes
                .iter()
                .any(|open| open.freq_hz == probe.freq_hz),
            "a known control channel stayed in the search"
        );
    }

    fn ruled_follower(
        engine: &Arc<Engine>,
        carrier: (u32, u32),
        talkgroups: TrunkTalkgroups,
    ) -> Follower {
        let mut follower = follower(engine, "tier_three", carrier);
        follower.systems[0].talkgroups = talkgroups;
        follower
    }

    fn grant_to(logical_channel: u16, slot: u8, destination: u32) -> DvFrame {
        DvFrame {
            channel: Some(logical_channel),
            destination: Some(destination),
            group_call: Some(true),
            ..grant(slot)
        }
    }

    fn define(follower: &mut Follower, carrier: (u32, u32), logical_channel: u16, rx_hz: u64) {
        let mut frame = definition(rx_hz);
        if let Some(definition) = &mut frame.channel_definition {
            definition.channel = logical_channel;
        }
        follower.observe(&record(carrier, frame));
    }

    /// Fills every receiver the system may hold with a call on its own channel and slot.
    fn fill(follower: &mut Follower, carrier: (u32, u32), center_hz: f64) {
        for logical_channel in 1..=(MAX_FOLLOWERS_PER_SYSTEM / 2) as u16 {
            let rx_hz = center_hz as u64 + u64::from(logical_channel) * 12_500;
            define(follower, carrier, logical_channel, rx_hz);
            for slot in [1, 2] {
                let destination = 100 + u32::from(logical_channel) * 2 + u32::from(slot);
                follower.observe(&record(
                    carrier,
                    grant_to(logical_channel, slot, destination),
                ));
            }
        }
        assert_eq!(follower.followers.len(), MAX_FOLLOWERS_PER_SYSTEM);
    }

    fn age_every_grant(follower: &mut Follower) {
        let past = Instant::now()
            .checked_sub(GRANT_HOLD)
            .expect("a clock older than the grant hold");
        for open in follower.followers.values_mut() {
            open.granted = past;
        }
    }

    #[test]
    fn a_locked_out_talkgroup_is_never_followed() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = ruled_follower(
            &engine,
            carrier,
            TrunkTalkgroups {
                lockout: vec![91],
                ..TrunkTalkgroups::default()
            },
        );

        follower.observe(&record(carrier, definition(center_hz as u64 + 125_000)));
        follower.observe(&record(carrier, grant(1)));

        assert!(follower.followers.is_empty());
    }

    #[test]
    fn listed_only_follows_just_the_priority_talkgroups() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = ruled_follower(
            &engine,
            carrier,
            TrunkTalkgroups {
                priority: vec![92],
                listed_only: true,
                ..TrunkTalkgroups::default()
            },
        );

        follower.observe(&record(carrier, definition(center_hz as u64 + 125_000)));
        follower.observe(&record(carrier, grant_to(LOGICAL_CHANNEL, 1, 91)));
        follower.observe(&record(carrier, grant_to(LOGICAL_CHANNEL, 2, 92)));

        let status = status(&follower);
        assert_eq!(status.followers.len(), 1);
        assert_eq!(status.followers[0].destination, Some(92));
    }

    #[test]
    fn a_follower_names_the_call_its_channel_was_granted_to() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = follower(&engine, "tier_three", carrier);

        follower.observe(&record(carrier, definition(center_hz as u64 + 125_000)));
        follower.observe(&record(carrier, identified_grant(1)));
        follower.observe(&record(carrier, grant_to(LOGICAL_CHANNEL, 1, 93)));

        let status = status(&follower);
        assert_eq!(status.followers.len(), 1);
        assert_eq!(status.followers[0].destination, Some(93));
        assert_eq!(status.followers[0].group_call, Some(true));
    }

    #[test]
    fn a_full_system_hands_an_idle_receiver_to_a_new_call() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = follower(&engine, "tier_three", carrier);
        fill(&mut follower, carrier, center_hz);
        age_every_grant(&mut follower);

        define(&mut follower, carrier, 40, center_hz as u64 + 125_000);
        follower.observe(&record(carrier, grant_to(40, 1, 7)));

        let status = status(&follower);
        assert_eq!(status.followers.len(), MAX_FOLLOWERS_PER_SYSTEM);
        assert!(
            status
                .followers
                .iter()
                .any(|open| open.destination == Some(7))
        );
        assert!(status.problems.is_empty());
    }

    #[test]
    fn a_priority_call_takes_a_busy_receiver_from_an_ordinary_one() {
        let engine = engine();
        let (device_set, channel, center_hz) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = ruled_follower(
            &engine,
            carrier,
            TrunkTalkgroups {
                priority: vec![7],
                ..TrunkTalkgroups::default()
            },
        );
        fill(&mut follower, carrier, center_hz);
        define(&mut follower, carrier, 40, center_hz as u64 + 125_000);
        define(&mut follower, carrier, 41, center_hz as u64 + 137_500);

        follower.observe(&record(carrier, grant_to(41, 1, 8)));
        follower.observe(&record(carrier, grant_to(40, 1, 7)));

        let status = status(&follower);
        assert_eq!(status.followers.len(), MAX_FOLLOWERS_PER_SYSTEM);
        assert!(
            status
                .followers
                .iter()
                .any(|open| open.destination == Some(7))
        );
        assert!(
            !status
                .followers
                .iter()
                .any(|open| open.destination == Some(8))
        );
        assert_eq!(
            status.problems.len(),
            1,
            "the ordinary call was not reported"
        );
    }

    #[test]
    fn a_control_burst_names_the_site() {
        let engine = engine();
        let (device_set, channel, _) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = follower(&engine, "tier_three", carrier);

        follower.observe(&record(
            carrier,
            DvFrame {
                system_id: Some(0x1234),
                network_id: Some(12),
                site_id: Some(3),
                ..grant(1)
            },
        ));

        let site = status(&follower).site;
        assert_eq!(site.system_id, Some(0x1234));
        assert_eq!(site.network_id, Some(12));
        assert_eq!(site.site_id, Some(3));
    }

    #[test]
    fn a_protocol_the_decoder_never_declared_is_not_taken_for_the_system() {
        let engine = engine();
        let (device_set, channel, _) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = follower(&engine, AUTO_TRUNK_PROTOCOL, carrier);

        follower.observe(&record(
            carrier,
            DvFrame {
                trunk_protocol: Some("phase_two".to_owned()),
                ..DvFrame::new("DMR", DvFrameKind::Control)
            },
        ));

        assert_eq!(status(&follower).detected, None);
    }

    #[test]
    fn a_quiet_control_channel_with_nowhere_else_to_go_stays_put() {
        let engine = engine();
        let (device_set, channel, _) = control_channel(&engine);
        let carrier = (device_set, channel);
        let mut follower = follower(&engine, "tier_three", carrier);
        let configured = follower.systems[0].radio.map(|radio| radio.control_hz);
        let silent = Instant::now()
            .checked_sub(CONTROL_SILENCE)
            .expect("a clock older than the silence");
        if let Some(control) = follower.controls.get_mut("trunk") {
            control.heard = silent;
        }

        follower.reconcile();

        assert_eq!(
            status(&follower).control.map(|control| control.freq_hz),
            configured
        );
    }

    #[test]
    fn a_silent_control_channel_steps_round_the_sites_others() {
        assert_eq!(next_control(10, 10, &[20, 30]), Some(20));
        assert_eq!(next_control(10, 20, &[20, 30]), Some(30));
        assert_eq!(next_control(10, 30, &[20, 30]), Some(10));
        assert_eq!(next_control(10, 10, &[10]), None);
        assert_eq!(next_control(10, 10, &[]), None);
    }
}

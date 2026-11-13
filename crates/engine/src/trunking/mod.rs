//! Follows trunked radio systems: hunts the control channel, opens a receiver on every traffic
//! channel a call is granted, and learns where the logical channels transmit. Which protocol a
//! system speaks is the decoder's business, declared through [`sdrmm_channels::Trunking`].

mod carriers;
mod control;
mod follow;
mod prospect;
mod search;
mod status;

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, Weak, atomic::AtomicU64, mpsc},
    time::{Duration, Instant},
};

use prospect::Prospector;
use sdrmm_channels::{TrunkGrant, Trunking};
use sdrmm_wire::{
    AUTO_TRUNK_PROTOCOL, DecodedRecord, DvFrame, DvFrameKind, StateScope, TrunkChannelEntry,
    TrunkDiscovery, TrunkKind, TrunkScheme, TrunkSite, TrunkSpec, TrunkSystemStatus,
    TrunkTalkgroups, VoiceKey,
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
    /// The serial port of the vocoder chip followed calls play through.
    pub vocoder: Option<String>,
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
    vocoder: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum FollowerKey {
    KnownCarrier {
        system_node: String,
        freq_hz: u64,
        slot: Option<u8>,
    },
    Granted {
        system_node: String,
        logical_channel: u16,
        slot: Option<u8>,
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

    fn slot(&self) -> Option<u8> {
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
    call: Option<TrunkGrant>,
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
    slot: Option<u8>,
    freq_hz: u64,
    call: Option<TrunkGrant>,
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
                    vocoder: system.vocoder.clone(),
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
        site.system_id = frame.system_id.map(u32::from).or(site.system_id);
        site.network_id = frame.network_id.or(site.network_id);
        site.site_id = frame.site_id.map(u32::from).or(site.site_id);
        if *site != before {
            self.publish();
        }
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
}

fn device_sets(live: &[Carrier]) -> Vec<u32> {
    let mut sets: Vec<u32> = live.iter().map(|carrier| carrier.device_set).collect();
    sets.sort_unstable();
    sets.dedup();
    sets
}

#[cfg(test)]
mod tests;

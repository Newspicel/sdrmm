use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use sdrmm_dsp::audio::{
    mix::{MixBus, lane_of},
    voice_gate::VoiceGate,
};
use sdrmm_voicechat::{FRAME, Heard, Talker};
use sdrmm_wire::{ChatTalker, VoiceChatNode, VoiceGateSettings};
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, MissedTickBehavior};

use super::{
    MIX_LAG, Tuning,
    gate::{gate, passes, scaled},
    status::Meters,
};

/// One frame's time: the pace silence goes out at once nobody talks.
const TICK: Duration = Duration::from_millis(20);
/// A talker unheard this long is no longer talking.
const TALKING: Duration = Duration::from_millis(400);
/// Ticks without a voice before the mix counts as quiet: one late frame is jitter.
const QUIET_TICKS: u32 = 2;

/// Delivers a frame to the transmitters the node keys; answers how many took it.
pub(super) type Air = Arc<dyn Fn(&[f32]) -> usize + Send + Sync>;

struct Seen {
    name: String,
    verified: bool,
    last: Instant,
}

/// Mixes the allowed talkers and gates the mix on its way to the air.
pub(super) struct ToAir {
    talkers: HashMap<String, Seen>,
    talkers_changed: bool,
    bus: MixBus,
    held: Vec<f32>,
    gate: VoiceGate,
    quiet: u32,
}

/// Whether a talker may key the transmitter: everyone when the list is empty, else a verified
/// talker on it, since a stand-in id is not a user.
pub(super) fn allowed(node: &VoiceChatNode, id: &str, verified: bool) -> bool {
    node.allowed.is_empty() || (verified && node.allowed.iter().any(|allowed| allowed == id))
}

impl ToAir {
    pub(super) fn new(settings: &VoiceGateSettings) -> Self {
        Self {
            talkers: HashMap::new(),
            talkers_changed: false,
            bus: MixBus::new(MIX_LAG),
            held: Vec::with_capacity(2 * FRAME),
            gate: gate(settings),
            quiet: 0,
        }
    }

    pub(super) fn hear(
        &mut self,
        heard: Heard,
        tuning: &Tuning,
        now: Instant,
        air: &Air,
        meters: &Meters,
    ) {
        let Heard { talker, pcm } = heard;
        let may_key = allowed(&tuning.node, &talker.id, talker.verified);
        self.note(&talker, now);
        if !tuning.keying || !may_key {
            return;
        }
        self.quiet = 0;
        self.bus.push(lane_of(&talker.id), &pcm);
        self.bus.drain(&mut self.held);
        self.release(&tuning.node.to_air, air, meters);
    }

    /// Runs once a frame: lets talkers go quiet and, while nobody talks, sends the rest of
    /// the mix and then silence through the gate, so its hang and Continuous keep the key.
    pub(super) fn tick(&mut self, tuning: &Tuning, now: Instant, air: &Air, meters: &Meters) {
        self.forget_quiet_talkers(now);
        if !tuning.keying {
            self.bus.flush(&mut self.held);
            self.held.clear();
            meters.air(None, false);
            return;
        }
        self.quiet = self.quiet.saturating_add(1);
        if self.quiet < QUIET_TICKS {
            return;
        }
        self.bus.flush(&mut self.held);
        let padded = self.held.len().div_ceil(FRAME).max(1) * FRAME;
        self.held.resize(padded, 0.0);
        self.release(&tuning.node.to_air, air, meters);
    }

    /// Who is talking now, each marked with whether the node lets them key.
    pub(super) fn talking(&self, node: &VoiceChatNode) -> Vec<ChatTalker> {
        let mut talking: Vec<ChatTalker> = self
            .talkers
            .iter()
            .map(|(id, seen)| ChatTalker {
                id: id.clone(),
                name: seen.name.clone(),
                allowed: allowed(node, id, seen.verified),
                verified: seen.verified,
            })
            .collect();
        talking.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        talking
    }

    fn publish(&mut self, node: &VoiceChatNode, retuned: bool, meters: &Meters) {
        if !self.talkers_changed && !retuned {
            return;
        }
        self.talkers_changed = false;
        let talking = self.talking(node);
        meters.talking.send_if_modified(|shown| {
            let changed = *shown != talking;
            *shown = talking;
            changed
        });
    }

    fn note(&mut self, talker: &Talker, now: Instant) {
        let Some(seen) = self.talkers.get_mut(&talker.id) else {
            let seen = Seen {
                name: talker.name.clone(),
                verified: talker.verified,
                last: now,
            };
            self.talkers.insert(talker.id.clone(), seen);
            self.talkers_changed = true;
            return;
        };
        seen.last = now;
        if seen.name != talker.name {
            seen.name.clone_from(&talker.name);
            self.talkers_changed = true;
        }
    }

    fn forget_quiet_talkers(&mut self, now: Instant) {
        let bus = &mut self.bus;
        let before = self.talkers.len();
        self.talkers.retain(|id, seen| {
            let talking = now.duration_since(seen.last) < TALKING;
            if !talking {
                bus.forget(lane_of(id));
            }
            talking
        });
        self.talkers_changed |= self.talkers.len() != before;
    }

    fn release(&mut self, settings: &VoiceGateSettings, air: &Air, meters: &Meters) {
        while self.held.len() >= FRAME {
            let frame = &self.held[..FRAME];
            let open = passes(&mut self.gate, settings, frame);
            meters.air(Some(self.gate.level_db()), open);
            if open {
                air(&scaled(frame, settings));
            }
            self.held.drain(..FRAME);
        }
    }
}

/// Hears the chat until the link closes; what the allowed talkers say goes to the air while
/// the node keys a transmitter.
pub(super) async fn run(
    mut heard: mpsc::Receiver<Heard>,
    mut tuning: watch::Receiver<Tuning>,
    meters: Arc<Meters>,
    air: Air,
) {
    let mut tuned = tuning.borrow_and_update().clone();
    let mut to_air = ToAir::new(&tuned.node.to_air);
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            got = heard.recv() => {
                let Some(got) = got else { return };
                to_air.hear(got, &tuned, Instant::now(), &air, &meters);
            }
            now = tick.tick() => to_air.tick(&tuned, now, &air, &meters),
        }
        let retuned = tuning.has_changed().unwrap_or(false);
        if retuned {
            tuned = tuning.borrow_and_update().clone();
        }
        to_air.publish(&tuned.node, retuned, &meters);
    }
}

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use sdrmm_engine::Engine;
use sdrmm_voicechat::LinkState;
use sdrmm_wire::{ChatTalker, ServerEvent, VoiceChatState, VoiceChatStatus};
use tokio::sync::{Notify, watch};
use tokio::time::MissedTickBehavior;

/// How often a joined node's levels go out while they move: meters at 10 Hz.
const LEVELS: Duration = Duration::from_millis(100);

/// One direction's level and gate, written by its audio task and read for the status.
pub(super) struct Gauge {
    db: AtomicU32,
    open: AtomicBool,
}

impl Default for Gauge {
    fn default() -> Self {
        Self {
            db: AtomicU32::new(f32::NAN.to_bits()),
            open: AtomicBool::new(false),
        }
    }
}

impl Gauge {
    fn set(&self, db: Option<f32>, open: bool) -> bool {
        self.db
            .store(db.unwrap_or(f32::NAN).to_bits(), Ordering::Relaxed);
        self.open.swap(open, Ordering::Relaxed) != open
    }

    pub(super) fn db(&self) -> Option<f32> {
        Some(f32::from_bits(self.db.load(Ordering::Relaxed))).filter(|db| !db.is_nan())
    }

    pub(super) fn open(&self) -> bool {
        self.open.load(Ordering::Relaxed)
    }
}

/// What a joined node shows besides its link state.
pub(super) struct Meters {
    pub(super) to_chat: Gauge,
    pub(super) to_air: Gauge,
    pub(super) talking: watch::Sender<Vec<ChatTalker>>,
    flipped: Notify,
}

impl Default for Meters {
    fn default() -> Self {
        Self {
            to_chat: Gauge::default(),
            to_air: Gauge::default(),
            talking: watch::Sender::new(Vec::new()),
            flipped: Notify::new(),
        }
    }
}

impl Meters {
    pub(super) fn chat(&self, db: Option<f32>, open: bool) {
        if self.to_chat.set(db, open) {
            self.flipped.notify_one();
        }
    }

    pub(super) fn air(&self, db: Option<f32>, open: bool) {
        if self.to_air.set(db, open) {
            self.flipped.notify_one();
        }
    }
}

pub(super) fn idle(node: &str, error: Option<String>) -> VoiceChatStatus {
    VoiceChatStatus {
        node: node.to_owned(),
        state: VoiceChatState::Idle,
        error,
        talking: Vec::new(),
        to_chat_db: None,
        to_air_db: None,
        to_chat_open: false,
        to_air_open: false,
    }
}

pub(super) fn joined(
    node: &str,
    link: &LinkState,
    ended: bool,
    meters: &Meters,
) -> VoiceChatStatus {
    let (state, error) = match link {
        LinkState::Connecting => (VoiceChatState::Connecting, None),
        LinkState::Joined => (VoiceChatState::Joined, None),
        LinkState::Failed(reason) if ended => (VoiceChatState::Idle, Some(reason.clone())),
        LinkState::Failed(reason) => (VoiceChatState::Connecting, Some(reason.clone())),
    };
    VoiceChatStatus {
        state,
        error,
        talking: meters.talking.borrow().clone(),
        to_chat_db: meters.to_chat.db(),
        to_air_db: meters.to_air.db(),
        to_chat_open: meters.to_chat.open(),
        to_air_open: meters.to_air.open(),
        ..idle(node, None)
    }
}

/// Publishes a joined node's status the moment its link, its talkers or a gate changes, and
/// its levels at most every [`LEVELS`] while they move.
pub(super) async fn watch(
    node: String,
    engine: Arc<Engine>,
    mut link: watch::Receiver<LinkState>,
    meters: Arc<Meters>,
) {
    let mut talking = meters.talking.subscribe();
    let mut levels = tokio::time::interval(LEVELS);
    levels.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut shown = None;
    loop {
        let metering = tokio::select! {
            changed = link.changed() => if changed.is_err() { return } else { false },
            changed = talking.changed() => if changed.is_err() { return } else { false },
            () = meters.flipped.notified() => false,
            _ = levels.tick() => true,
        };
        let status = joined(&node, &link.borrow_and_update(), false, &meters);
        let read = (status.to_chat_db, status.to_air_db);
        if metering && (status.state != VoiceChatState::Joined || shown == Some(read)) {
            continue;
        }
        shown = Some(read);
        talking.mark_unchanged();
        engine.emit_event(ServerEvent::VoiceChatStatus(status));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn moving_levels_go_out_at_ten_hertz() {
        let engine = Engine::new(None);
        let mut events = engine.subscribe_events();
        let (_state, link) = watch::channel(LinkState::Joined);
        let meters = Arc::new(Meters::default());
        let watcher = tokio::spawn(watch("vc".into(), engine.clone(), link, meters.clone()));
        let start = tokio::time::Instant::now();
        let mut level = -60.0;
        while start.elapsed() < Duration::from_millis(500) {
            level += 0.1;
            meters.air(Some(level), false);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        watcher.abort();
        let mut sent = 0;
        while let Ok(event) = events.try_recv() {
            if let ServerEvent::VoiceChatStatus(status) = event {
                assert_eq!(status.node, "vc");
                sent += 1;
            }
        }
        assert!((3..=8).contains(&sent), "{sent} statuses in 500 ms");
    }

    #[tokio::test(start_paused = true)]
    async fn still_levels_send_nothing() {
        let engine = Engine::new(None);
        let mut events = engine.subscribe_events();
        let (_state, link) = watch::channel(LinkState::Joined);
        let meters = Arc::new(Meters::default());
        meters.air(Some(-30.0), false);
        let watcher = tokio::spawn(watch("vc".into(), engine.clone(), link, meters));
        tokio::time::sleep(Duration::from_secs(1)).await;
        watcher.abort();
        let mut sent = 0;
        while let Ok(event) = events.try_recv() {
            sent += usize::from(matches!(event, ServerEvent::VoiceChatStatus(_)));
        }
        assert_eq!(sent, 1);
    }
}

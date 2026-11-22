//! The channel side of a joined voice channel, and what both services share around it.

use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, timeout_at};

use crate::{Heard, LinkState};

/// Frames either way may queue up to 1.28 s before the sender waits or a heard frame drops.
const QUEUE: usize = 64;
/// Silence on `send` this long ends a talk spurt.
const IDLE: Duration = Duration::from_millis(200);
const FIRST_RETRY: Duration = Duration::from_secs(1);
const LAST_RETRY: Duration = Duration::from_secs(60);

/// One joined voice channel as plain channels of 20 ms mono frames at 48 kHz.
///
/// Dropping the link, its `hold` or its `heard` end leaves the channel.
#[derive(Debug)]
pub struct Link {
    pub send: mpsc::Sender<Vec<f32>>,
    pub heard: mpsc::Receiver<Heard>,
    pub state: watch::Receiver<LinkState>,
    /// Heard frames lost so far: `heard` full, or a frame that would not decode.
    pub drops: Arc<AtomicU64>,
    pub hold: Hold,
}

/// Keeps a link's seat. Dropping it frees the seat at once, while the tasks reading `heard`
/// may still be winding down, so a link joined right after does not clash with it.
#[derive(Debug)]
pub struct Hold {
    _seat: watch::Sender<()>,
}

/// A hold no seat watches, for a link made outside this crate.
impl Default for Hold {
    fn default() -> Self {
        Self {
            _seat: watch::Sender::new(()),
        }
    }
}

impl Link {
    pub fn dropped(&self) -> u64 {
        self.drops.load(Ordering::Relaxed)
    }
}

/// The connection's ends of a [`Link`].
pub(crate) struct Ends {
    pub(crate) outbox: Outbox,
    pub(crate) inbox: Inbox,
}

pub(crate) fn link() -> (Link, Ends) {
    let (send, outgoing) = mpsc::channel(QUEUE);
    let (heard_tx, heard) = mpsc::channel(QUEUE);
    let (state_tx, state) = watch::channel(LinkState::Connecting);
    let (hold, held) = watch::channel(());
    let drops = Arc::new(AtomicU64::new(0));
    let ends = Ends {
        outbox: Outbox {
            frames: outgoing,
            idle_at: None,
            closed: false,
        },
        inbox: Inbox {
            ear: Ear {
                heard: heard_tx,
                drops: Arc::clone(&drops),
                dropping: Arc::new(AtomicBool::new(false)),
            },
            state: state_tx,
            held,
        },
    };
    let link = Link {
        send,
        heard,
        state,
        drops,
        hold: Hold { _seat: hold },
    };
    (link, ends)
}

/// What the link asks the connection to say next.
#[derive(Debug, PartialEq)]
pub(crate) enum Outgoing {
    Frame(Vec<f32>),
    /// Nothing came for a while: end the talk spurt.
    Idle,
}

pub(crate) struct Outbox {
    frames: mpsc::Receiver<Vec<f32>>,
    idle_at: Option<Instant>,
    closed: bool,
}

impl Outbox {
    /// The next frame, or one [`Outgoing::Idle`] after a spurt. Pends forever once `send` is
    /// gone, so a link kept only for listening stays joined.
    pub(crate) async fn next(&mut self) -> Outgoing {
        if self.closed {
            return std::future::pending().await;
        }
        let frame = if let Some(deadline) = self.idle_at {
            let Ok(frame) = timeout_at(deadline, self.frames.recv()).await else {
                self.idle_at = None;
                return Outgoing::Idle;
            };
            frame
        } else {
            self.frames.recv().await
        };
        if let Some(frame) = frame {
            self.idle_at = Some(Instant::now() + IDLE);
            Outgoing::Frame(frame)
        } else {
            self.closed = true;
            self.idle_at = None;
            Outgoing::Idle
        }
    }

    /// Throws frames away while the link is not in a channel, so `send` never backs up and
    /// nothing stale goes out once it is.
    pub(crate) async fn discard(&mut self) -> Infallible {
        while self.frames.recv().await.is_some() {}
        self.closed = true;
        std::future::pending().await
    }

    /// Forgets what queued before the link joined.
    pub(crate) fn clear(&mut self) {
        while self.frames.try_recv().is_ok() {}
        self.idle_at = None;
    }
}

pub(crate) struct Inbox {
    ear: Ear,
    state: watch::Sender<LinkState>,
    held: watch::Receiver<()>,
}

impl Inbox {
    pub(crate) fn set(&self, state: LinkState) {
        self.state.send_if_modified(|current| {
            let changed = *current != state;
            *current = state;
            changed
        });
    }

    pub(crate) fn hear(&self, heard: Heard) {
        self.ear.hear(heard);
    }

    pub(crate) fn ear(&self) -> Ear {
        self.ear.clone()
    }

    pub(crate) fn lose(&self, reason: &'static str) {
        self.ear.lose(reason);
    }

    pub(crate) async fn released(&self) {
        let mut held = self.held.clone();
        tokio::select! {
            () = self.ear.heard.closed() => {}
            () = async { while held.changed().await.is_ok() {} } => {}
        }
    }

    pub(crate) fn seat(&self, place: String) -> Seat {
        Seat {
            place,
            heard: self.ear.heard.downgrade(),
            held: self.held.clone(),
        }
    }
}

/// Where heard frames go, counting the ones that never arrive.
#[derive(Clone)]
pub(crate) struct Ear {
    heard: mpsc::Sender<Heard>,
    drops: Arc<AtomicU64>,
    dropping: Arc<AtomicBool>,
}

impl Ear {
    /// Hands a heard frame over. A full queue drops it, counted: a link nobody reads must not
    /// stall the connection.
    pub(crate) fn hear(&self, heard: Heard) {
        match self.heard.try_send(heard) {
            Ok(()) => self.dropping.store(false, Ordering::Relaxed),
            Err(TrySendError::Full(_)) => self.lose("heard queue full"),
            Err(TrySendError::Closed(_)) => {}
        }
    }

    /// Counts a lost heard frame, warning when a run of losses starts.
    pub(crate) fn lose(&self, reason: &'static str) {
        self.drops.fetch_add(1, Ordering::Relaxed);
        if !self.dropping.swap(true, Ordering::Relaxed) {
            tracing::warn!(reason, "voice chat frames dropped");
        }
    }
}

/// Where a link sits, held while the link is alive.
pub(crate) struct Seat {
    pub(crate) place: String,
    heard: mpsc::WeakSender<Heard>,
    held: watch::Receiver<()>,
}

impl Seat {
    pub(crate) fn held(&self) -> bool {
        self.held.has_changed().is_ok()
            && self.heard.upgrade().is_some_and(|heard| !heard.is_closed())
    }

    pub(crate) fn belongs_to(&self, inbox: &Inbox) -> bool {
        self.heard
            .upgrade()
            .is_some_and(|heard| heard.same_channel(&inbox.ear.heard))
    }
}

/// Waits between reconnects: 1 s, doubling to 60 s, back to 1 s once joined.
pub(crate) struct Backoff {
    next: Duration,
}

impl Backoff {
    pub(crate) fn new() -> Self {
        Self { next: FIRST_RETRY }
    }

    pub(crate) fn reset(&mut self) {
        self.next = FIRST_RETRY;
    }

    pub(crate) fn step(&mut self) -> Duration {
        let wait = self.next;
        self.next = (self.next * 2).min(LAST_RETRY);
        wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_to_a_minute() {
        let mut backoff = Backoff::new();
        let waits: Vec<u64> = (0..8).map(|_| backoff.step().as_secs()).collect();
        assert_eq!(waits, [1, 2, 4, 8, 16, 32, 60, 60]);
        backoff.reset();
        assert_eq!(backoff.step(), FIRST_RETRY);
    }

    #[tokio::test(start_paused = true)]
    async fn a_spurt_ends_once_after_silence() {
        let (link, mut ends) = link();
        link.send.send(vec![0.5; 4]).await.unwrap();
        assert_eq!(ends.outbox.next().await, Outgoing::Frame(vec![0.5; 4]));
        assert_eq!(ends.outbox.next().await, Outgoing::Idle);
        let quiet = tokio::time::timeout(Duration::from_secs(5), ends.outbox.next()).await;
        assert!(quiet.is_err());
    }

    #[tokio::test]
    async fn dropping_the_link_frees_its_seat() {
        let (link, ends) = link();
        let seat = ends.inbox.seat("Lobby".into());
        assert!(seat.held());
        drop(link);
        assert!(!seat.held());
        ends.inbox.released().await;
    }

    #[tokio::test]
    async fn dropping_the_hold_frees_the_seat_while_heard_lives() {
        let (link, ends) = link();
        let seat = ends.inbox.seat("Lobby".into());
        let Link { heard, hold, .. } = link;
        drop(hold);
        assert!(!seat.held());
        assert!(seat.belongs_to(&ends.inbox));
        ends.inbox.released().await;
        drop(heard);
    }

    #[tokio::test]
    async fn lost_heard_frames_are_counted() {
        let (link, ends) = link();
        let frame = || Heard {
            talker: crate::Talker {
                id: "1".into(),
                name: "Ann".into(),
                verified: true,
            },
            pcm: vec![0.0; 4],
        };
        for _ in 0..QUEUE + 2 {
            ends.inbox.hear(frame());
        }
        ends.inbox.lose("undecodable");
        assert_eq!(link.dropped(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn frames_sent_before_joining_never_go_out() {
        let (link, mut ends) = link();
        for _ in 0..QUEUE {
            link.send.send(vec![0.1; 4]).await.unwrap();
        }
        let waited = tokio::time::timeout(Duration::from_secs(1), ends.outbox.discard()).await;
        assert!(waited.is_err());
        link.send.try_send(vec![0.2; 4]).unwrap();
        ends.outbox.clear();
        link.send.send(vec![0.3; 4]).await.unwrap();
        assert_eq!(ends.outbox.next().await, Outgoing::Frame(vec![0.3; 4]));
    }

    #[tokio::test]
    async fn state_follows_the_connection() {
        let (link, ends) = link();
        assert_eq!(*link.state.borrow(), LinkState::Connecting);
        ends.inbox.set(LinkState::Joined);
        assert_eq!(*link.state.borrow(), LinkState::Joined);
    }
}

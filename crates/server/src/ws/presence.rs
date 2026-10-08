use std::time::Instant;

use sdrmm_wire::{POINTER_BURST, POINTER_RATE_HZ, Pointer, ServerEvent, valid_author};
use tokio::sync::broadcast;

use super::{LIMIT_ERROR_EVERY, Outbox, Session, phone::RateBudget, text_event};
use crate::presence::PresenceEvent;

const BAD_AUTHOR: &str = "Present needs a short author key";
const NOT_PRESENT: &str = "Send Present before Point";
const POINTER_TOO_BIG: &str = "Pointer names too many nodes";
const POINTER_TOO_FAST: &str = "Pointer updates are limited to 30 Hz";

pub(super) struct PeerLink {
    pub(super) id: u32,
    fallback: String,
    task: Option<tokio::task::JoinHandle<()>>,
    budget: RateBudget,
    last_limit_error: Option<Instant>,
}

impl PeerLink {
    pub(super) fn new(id: u32, fallback: String) -> Self {
        Self {
            id,
            fallback,
            task: None,
            budget: RateBudget::new(f64::from(POINTER_BURST), f64::from(POINTER_RATE_HZ)),
            last_limit_error: None,
        }
    }

    pub(super) fn abort(self) {
        if let Some(task) = self.task {
            task.abort();
        }
    }
}

impl Session {
    pub(super) async fn present(&mut self, author: &str, name: &str) {
        if !valid_author(author) {
            self.send_error(BAD_AUTHOR).await;
            return;
        }
        let presence = self.state.presence.clone();
        if self.peer.task.is_none() {
            self.peer.task = Some(spawn_presence(
                presence.subscribe(),
                self.out.clone(),
                self.peer.id,
                presence.clone(),
            ));
        }
        presence.present(self.peer.id, author, name, &self.peer.fallback);
    }

    pub(super) async fn point(&mut self, pointer: Pointer) {
        if !pointer.fits() {
            self.send_error(POINTER_TOO_BIG).await;
            return;
        }
        if !self.peer.budget.take() {
            let now = Instant::now();
            if self
                .peer
                .last_limit_error
                .is_none_or(|last| now.duration_since(last) >= LIMIT_ERROR_EVERY)
            {
                self.peer.last_limit_error = Some(now);
                self.send_error(POINTER_TOO_FAST).await;
            }
            return;
        }
        if !self.state.presence.point(self.peer.id, pointer) {
            self.send_error(NOT_PRESENT).await;
        }
    }
}

fn spawn_presence(
    mut events: broadcast::Receiver<PresenceEvent>,
    out: Outbox,
    me: u32,
    presence: std::sync::Arc<crate::presence::Presence>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let sent = match events.recv().await {
                Ok(PresenceEvent::Peers(peers)) => {
                    let listed = ServerEvent::Peers {
                        you: me,
                        peers: peers.to_vec(),
                    };
                    out.send(text_event(&listed)).await
                }
                Ok(PresenceEvent::Pointer { peer, pointer }) if peer != me => {
                    let moved = ServerEvent::PeerPointer {
                        peer,
                        pointer: (*pointer).clone(),
                    };
                    out.send_pointer(peer, text_event(&moved))
                }
                Ok(PresenceEvent::Pointer { .. }) => Ok(()),
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let listed = ServerEvent::Peers {
                        you: me,
                        peers: presence.peers().to_vec(),
                    };
                    out.send(text_event(&listed)).await
                }
                Err(broadcast::error::RecvError::Closed) => break,
            };
            if sent.is_err() {
                break;
            }
        }
    })
}

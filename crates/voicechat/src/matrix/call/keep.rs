//! Keeps our membership alive: the delayed leave restarted, `expires` extended.

use std::time::Duration;

use futures::future::BoxFuture;
use tokio::time::Instant;

use super::homeserver::{Homeserver, Restart};
use super::run::{DELAYED_LEAVE, Presence, Stop};
use crate::matrix::membership::{EXPIRES_MS, StateEntry, extend};

/// How long before our membership lapses it is extended.
const EXPIRY_HEADROOM: Duration = Duration::from_secs(60);
/// How soon a failed restart is tried again.
const RETRY: Duration = Duration::from_secs(1);
const EXPIRES: Duration = Duration::from_millis(EXPIRES_MS);

/// A finished keep-alive request.
pub(super) enum Kept {
    Restarted(Result<(), Restart>),
    /// A new delayed leave, then our membership again.
    Rescheduled(Result<String, String>, Result<(), String>),
    Extended(Result<(), String>),
}

pub(super) struct Keeper {
    entry: StateEntry,
    /// One restart or reschedule at a time, so none outlives its successor.
    restarting: bool,
    /// When the server last confirmed our delayed leave.
    kept: Instant,
    extend_at: Instant,
    /// The extended membership has not reached the server yet.
    stale: bool,
    putting: bool,
}

fn restart<H: Homeserver>(hs: &H, id: String, wait: Duration) -> BoxFuture<'_, Kept> {
    Box::pin(async move {
        tokio::time::sleep(wait).await;
        Kept::Restarted(hs.restart_delayed(&id).await)
    })
}

fn reschedule<H: Homeserver>(hs: &H, leave: StateEntry, entry: StateEntry) -> BoxFuture<'_, Kept> {
    Box::pin(async move {
        let delay = hs.put_delayed(&leave, DELAYED_LEAVE).await;
        let put = match &delay {
            Ok(_) => hs.put_state(&entry).await,
            Err(_) => Ok(()),
        };
        Kept::Rescheduled(delay, put)
    })
}

fn lost() -> Stop {
    Stop::Lost("Membership lost".to_owned())
}

impl Keeper {
    pub(super) fn new(entry: StateEntry) -> Self {
        let now = Instant::now();
        Self {
            entry,
            restarting: false,
            kept: now,
            extend_at: now + EXPIRES - EXPIRY_HEADROOM,
            stale: false,
            putting: false,
        }
    }

    /// The requests due at a tick.
    pub(super) fn tick<'a, H: Homeserver>(
        &mut self,
        hs: &'a H,
        presence: &Presence,
    ) -> Vec<BoxFuture<'a, Kept>> {
        let mut due = Vec::new();
        if !self.restarting
            && let Some(id) = presence.delay.clone()
        {
            self.restarting = true;
            due.push(restart(hs, id, Duration::ZERO));
        }
        if Instant::now() >= self.extend_at {
            extend(&mut self.entry);
            self.extend_at += EXPIRES;
            self.stale = true;
        }
        if self.stale && !self.putting {
            self.putting = true;
            let entry = self.entry.clone();
            due.push(Box::pin(async move {
                Kept::Extended(hs.put_state(&entry).await)
            }));
        }
        due
    }

    /// Takes a finished request, answering the next one. A transient failure retries the same
    /// delayed leave; only a server that lost it gets a new one, as js-sdk `MembershipManager`
    /// does. Once the old one may have fired, the call rejoins.
    pub(super) fn done<'a, H: Homeserver>(
        &mut self,
        hs: &'a H,
        presence: &mut Presence,
        kept: Kept,
    ) -> Result<Option<BoxFuture<'a, Kept>>, Stop> {
        match kept {
            Kept::Restarted(Ok(())) => {
                self.restarting = false;
                self.kept = Instant::now();
                Ok(None)
            }
            Kept::Restarted(Err(Restart::Gone)) => {
                let leave = presence.leave.clone().ok_or_else(lost)?;
                Ok(Some(reschedule(hs, leave, self.entry.clone())))
            }
            Kept::Restarted(Err(Restart::Failed(reason))) => {
                tracing::warn!(reason, "delayed leave not restarted");
                let id = presence.delay.clone().ok_or_else(lost)?;
                if self.kept.elapsed() + RETRY >= DELAYED_LEAVE {
                    return Err(lost());
                }
                Ok(Some(restart(hs, id, RETRY)))
            }
            Kept::Rescheduled(delay, put) => {
                self.restarting = false;
                let id = delay.map_err(Stop::Lost)?;
                presence.delay = Some(id);
                put.map_err(Stop::Lost)?;
                self.kept = Instant::now();
                Ok(None)
            }
            Kept::Extended(put) => {
                self.putting = false;
                match put {
                    Ok(()) => self.stale = false,
                    Err(reason) => tracing::warn!(reason, "membership not extended"),
                }
                Ok(None)
            }
        }
    }
}

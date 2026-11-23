//! One call from joining to leaving, retried with backoff until its link is released.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::broadcast::{self, error::RecvError};

use super::homeserver::Homeserver;
use super::live::Live;
use super::media::Media;
use super::people::{Me, People};
use super::wire::{Dial, Role};
use super::{Ended, SyncEvent};
use crate::LinkState;
use crate::link::{Backoff, Ear, Ends, Inbox};
use crate::matrix::e2ee::keys::EncryptionKeys;
use crate::matrix::membership::{StateEntry, our_leave, our_membership};

pub(super) const DELAYED_LEAVE: Duration = Duration::from_secs(8);
const SYNC_STOPPED: &str = "Sync stopped";

pub(super) enum Stop {
    Released,
    Lost(String),
    /// The session's sync is gone.
    Closed,
}

/// What a call keeps across reconnects: who is in, their keys and our audio.
pub(super) struct Call {
    pub(super) people: People,
    pub(super) media: Media,
}

/// Our membership as the homeserver holds it, so leaving can undo it.
#[derive(Default)]
pub(super) struct Presence {
    pub(super) leave: Option<StateEntry>,
    pub(super) delay: Option<String>,
}

impl Presence {
    async fn clear(&mut self, hs: &impl Homeserver) {
        if let Some(leave) = self.leave.take()
            && let Err(reason) = hs.put_state(&leave).await
        {
            tracing::warn!(reason, "membership not cleared");
        }
        if let Some(id) = self.delay.take()
            && let Err(reason) = hs.cancel_delayed(&id).await
        {
            tracing::debug!(reason, "delayed leave not cancelled");
        }
    }
}

/// The homeserver, the dialer and who we are.
pub(super) struct Line<'a, H, D> {
    pub(super) hs: &'a H,
    pub(super) dial: &'a D,
    pub(super) me: &'a Me,
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| u64::try_from(since.as_millis()).ok())
        .unwrap_or_default()
}

pub(super) async fn run<H: Homeserver, D: Dial>(
    hs: &H,
    dial: &D,
    me: Me,
    mut events: broadcast::Receiver<SyncEvent>,
    ends: &mut Ends,
) -> Ended {
    let line = Line { hs, dial, me: &me };
    let mut backoff = Backoff::new();
    let mut call = None;
    loop {
        let stop = attempt(&line, &mut call, &mut events, ends, &mut backoff).await;
        if let Some(ended) = finish(stop, &ends.inbox) {
            return ended;
        }
        let waited = wait_out(backoff.step(), call.as_mut(), &mut events, ends).await;
        if let Some(ended) = waited.and_then(|stop| finish(stop, &ends.inbox)) {
            return ended;
        }
    }
}

fn finish(stop: Stop, inbox: &Inbox) -> Option<Ended> {
    match stop {
        Stop::Released => Some(Ended::Released),
        Stop::Closed => {
            inbox.set(LinkState::Failed(SYNC_STOPPED.to_owned()));
            Some(Ended::Failed(SYNC_STOPPED.to_owned()))
        }
        Stop::Lost(reason) => {
            inbox.set(LinkState::Failed(reason));
            None
        }
    }
}

/// Sits out a backoff, still taking the keys others send.
async fn wait_out(
    delay: Duration,
    mut call: Option<&mut Call>,
    events: &mut broadcast::Receiver<SyncEvent>,
    ends: &mut Ends,
) -> Option<Stop> {
    let sleep = tokio::time::sleep(delay);
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            () = &mut sleep => return None,
            () = ends.inbox.released() => return Some(Stop::Released),
            never = ends.outbox.discard() => match never {},
            event = events.recv() => match event {
                Ok(SyncEvent::Key { sender, device, content }) => {
                    if let Some(call) = call.as_deref_mut() {
                        take_key(call, &sender, device.as_deref(), &content);
                    }
                }
                Ok(SyncEvent::Member { .. }) => {}
                Err(RecvError::Lagged(missed)) => tracing::warn!(missed, "sync events missed"),
                Err(RecvError::Closed) => return Some(Stop::Closed),
            },
        }
    }
}

/// Files a received key under every identity its sender may have in `LiveKit`.
pub(super) fn take_key(
    call: &mut Call,
    sender: &str,
    device: Option<&str>,
    content: &EncryptionKeys,
) {
    let ids = call.people.key_from(sender, device, content);
    let (Some(keys), Some((index, material))) = (call.media.keys(), content.entry()) else {
        return;
    };
    for id in ids {
        if let Err(error) = keys.receive(&id, index, &material) {
            tracing::warn!(%error, sender, "media key not taken");
        }
    }
}

async fn attempt<H: Homeserver, D: Dial>(
    line: &Line<'_, H, D>,
    slot: &mut Option<Call>,
    events: &mut broadcast::Receiver<SyncEvent>,
    ends: &mut Ends,
    backoff: &mut Backoff,
) -> Stop {
    let mut presence = Presence::default();
    let opened = tokio::select! {
        opened = open(line, slot, &mut presence, ends.inbox.ear()) => opened,
        () = ends.inbox.released() => Err(Stop::Released),
        never = ends.outbox.discard() => match never {},
    };
    let stop = match opened {
        Ok((mut live, call)) => {
            ends.outbox.clear();
            ends.inbox.set(LinkState::Connecting);
            backoff.reset();
            let stop = live.run(call, &mut presence, events, ends).await;
            live.leave().await;
            stop
        }
        Err(stop) => stop,
    };
    presence.clear(line.hs).await;
    stop
}

async fn new_call<H: Homeserver, D>(line: &Line<'_, H, D>, ear: Ear) -> Result<Call, Stop> {
    let encrypted = line.hs.encrypted().await.map_err(Stop::Lost)?;
    Ok(Call {
        people: People::new(line.me.clone(), line.hs.room_id(), encrypted),
        media: Media::new(encrypted, ear, Instant::now()).map_err(Stop::Lost)?,
    })
}

/// Focus, token, membership, then the publishing wire. The delayed leave goes out before the
/// membership, as js-sdk `MembershipManager` sends it, so a crash leaves no ghost.
async fn open<'a, 'c, H: Homeserver, D: Dial>(
    line: &Line<'a, H, D>,
    slot: &'c mut Option<Call>,
    presence: &mut Presence,
    ear: Ear,
) -> Result<(Live<'a, H, D>, &'c mut Call), Stop> {
    let Line { hs, dial, me } = *line;
    let foci = hs.foci().await.map_err(Stop::Lost)?;
    let ours = foci
        .into_iter()
        .next()
        .ok_or_else(|| Stop::Lost("No call server".to_owned()))?;
    let call = match slot.take() {
        Some(call) => call,
        None => new_call(line, ear).await?,
    };
    let call = slot.insert(call);
    call.people
        .seed(hs.call_states().await.map_err(Stop::Lost)?, now_ms());
    let grant = dial.grant(&ours).await.map_err(Stop::Lost)?;
    let owned = hs.owned_state_keys();
    let leave = our_leave(&me.user, &me.device, owned);
    if hs.delayed_events().await {
        match hs.put_delayed(&leave, DELAYED_LEAVE).await {
            Ok(id) => presence.delay = Some(id),
            Err(reason) => tracing::warn!(reason, "no delayed leave"),
        }
    }
    let entry = our_membership(&me.user, &me.device, &ours, now_ms(), owned);
    presence.leave = Some(leave);
    hs.put_state(&entry).await.map_err(Stop::Lost)?;
    let encrypted = call.people.encrypted();
    let own = dial
        .connect(&grant, Role::Talk { encrypted })
        .await
        .map_err(Stop::Lost)?;
    Ok((Live::new(hs, dial, ours, own, entry), call))
}

//! A Matrix account: one client and one sync per session, one call per joined room.

pub(super) mod connect;
pub(super) mod sync;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use matrix_sdk::ruma::{OwnedRoomId, RoomAliasId, RoomId, RoomOrAliasId};
use matrix_sdk::{Client, Room, RoomState};
use tokio::runtime::Handle;
use tokio::sync::{broadcast, mpsc, watch};

use super::MatrixAccount;
use super::call::{self, CallContext, Ended, SyncEvent};
use crate::link::{self, Backoff, Ends, Inbox, Seat};
use crate::{Link, LinkState, VoiceChatError, lock};

const ROOM_NOT_FOUND: &str = "Room not found";

/// How the session's client stands, as its links see it.
#[derive(Clone)]
enum Status {
    Starting,
    Ready(Arc<Ready>),
    Failed(String),
}

/// A signed-in client whose sync runs.
struct Ready {
    client: Client,
    http: reqwest::Client,
    /// Weak, so the feed closes once the sync stops, however long a link keeps this.
    events: broadcast::WeakSender<SyncEvent>,
}

pub(crate) struct Matrix {
    account: Arc<MatrixAccount>,
    /// Followed by the client, which sets the display name without leaving any call.
    nickname: watch::Sender<String>,
    saved: watch::Sender<Option<String>>,
    status: Mutex<Option<watch::Receiver<Status>>>,
    rooms: Arc<Rooms>,
    alive: mpsc::Sender<()>,
    _alive: mpsc::Receiver<()>,
}

impl Matrix {
    pub(crate) fn new(account: MatrixAccount) -> Self {
        let (alive, keep) = mpsc::channel(1);
        Self {
            nickname: watch::Sender::new(account.nickname.clone()),
            account: Arc::new(account),
            saved: watch::Sender::new(None),
            status: Mutex::new(None),
            rooms: Arc::default(),
            alive,
            _alive: keep,
        }
    }

    pub(crate) fn saved(&self) -> watch::Receiver<Option<String>> {
        self.saved.subscribe()
    }

    /// The session this backend runs on: the last one it saved, else the one it started with.
    pub(crate) fn latest(&self) -> Option<String> {
        self.saved
            .borrow()
            .clone()
            .or_else(|| self.account.session.clone())
    }

    pub(crate) fn rename(&self, nickname: String) {
        self.nickname.send_replace(nickname);
    }

    pub(crate) fn join(&self, runtime: &Handle, room: &str) -> Result<Link, VoiceChatError> {
        let room = room.trim();
        if room.is_empty() {
            return Err(VoiceChatError::NoChannel);
        }
        if !room.starts_with(['!', '#']) {
            return Err(VoiceChatError::BadChannel(room.to_owned()));
        }
        let (link, ends) = link::link();
        if let Some(taken) = self.rooms.claim(room, room, &ends.inbox) {
            return Err(VoiceChatError::Clash(taken));
        }
        let job = Job {
            status: self.status(runtime),
            rooms: Arc::clone(&self.rooms),
            room: room.to_owned(),
            alive: self.alive.clone(),
        };
        runtime.spawn(job.run(ends));
        Ok(link)
    }

    /// The account's status, its client started by the first join.
    fn status(&self, runtime: &Handle) -> watch::Receiver<Status> {
        let mut slot = lock(&self.status);
        if let Some(status) = slot.as_ref() {
            return status.clone();
        }
        let (status, watched) = watch::channel(Status::Starting);
        let line = sync::Line {
            account: Arc::clone(&self.account),
            status,
            saved: self.saved.clone(),
            names: self.nickname.subscribe(),
        };
        runtime.spawn(sync::run_account(line, self.alive.clone()));
        *slot = Some(watched.clone());
        watched
    }
}

/// The rooms the session's links sit in, by what they asked for and by room id.
#[derive(Default)]
struct Rooms(Mutex<HashMap<String, Seat>>);

impl Rooms {
    /// Seats `inbox`'s link under `key`, shown as `place`, or names the room another link
    /// holds there.
    fn claim(&self, key: &str, place: &str, inbox: &Inbox) -> Option<String> {
        let mut rooms = lock(&self.0);
        rooms.retain(|_, seat| seat.held());
        if let Some(seat) = rooms.get(key).filter(|seat| !seat.belongs_to(inbox)) {
            return Some(seat.place.clone());
        }
        rooms.insert(key.to_owned(), inbox.seat(place.to_owned()));
        None
    }
}

/// Why a call could not start.
enum Refusal {
    /// Another link sits in the room; retrying cannot help.
    Clash(String),
    Failed(String),
}

/// One link's room, joined whenever the account is ready.
struct Job {
    status: watch::Receiver<Status>,
    rooms: Arc<Rooms>,
    room: String,
    alive: mpsc::Sender<()>,
}

impl Job {
    async fn run(mut self, mut ends: Ends) {
        let mut backoff = Backoff::new();
        loop {
            let ready = tokio::select! {
                ready = ready(&mut self.status, &ends.inbox) => ready,
                () = ends.inbox.released() => return,
                () = self.alive.closed() => return,
                never = ends.outbox.discard() => match never {},
            };
            let Some((ready, events)) = ready else {
                return ends.inbox.released().await;
            };
            let opened = tokio::select! {
                opened = self.open(&ready, events, &ends.inbox) => opened,
                () = ends.inbox.released() => return,
                never = ends.outbox.discard() => match never {},
            };
            drop(ready);
            match opened {
                Ok(ctx) => {
                    backoff.reset();
                    if call::run_call(ctx, &mut ends).await == Ended::Released {
                        return;
                    }
                }
                Err(Refusal::Clash(taken)) => {
                    ends.inbox
                        .set(LinkState::Failed(format!("already in {taken}")));
                    return ends.inbox.released().await;
                }
                Err(Refusal::Failed(reason)) => {
                    ends.inbox.set(LinkState::Failed(reason));
                    tokio::select! {
                        () = tokio::time::sleep(backoff.step()) => {}
                        () = ends.inbox.released() => return,
                        () = self.alive.closed() => return,
                        never = ends.outbox.discard() => match never {},
                    }
                }
            }
        }
    }

    /// The room joined and everything its call needs.
    async fn open(
        &self,
        ready: &Ready,
        events: broadcast::Receiver<SyncEvent>,
        inbox: &Inbox,
    ) -> Result<CallContext, Refusal> {
        let client = &ready.client;
        let id = find(client, &self.room).await.map_err(Refusal::Failed)?;
        if let Some(taken) = self.rooms.claim(id.as_str(), &self.room, inbox) {
            return Err(Refusal::Clash(taken));
        }
        let room = enter(client, &id, &self.room)
            .await
            .map_err(Refusal::Failed)?;
        let (Some(user), Some(device)) = (client.user_id(), client.device_id()) else {
            return Err(Refusal::Failed(connect::NOT_SIGNED_IN.to_owned()));
        };
        Ok(CallContext {
            client: client.clone(),
            room,
            http: ready.http.clone(),
            user: user.to_string(),
            device: device.to_string(),
            events,
        })
    }
}

/// The account once it is ready, with a fresh feed of its sync. Until then the link shows
/// how the account stands. None once the account is gone for good.
async fn ready(
    status: &mut watch::Receiver<Status>,
    inbox: &Inbox,
) -> Option<(Arc<Ready>, broadcast::Receiver<SyncEvent>)> {
    loop {
        let current = status.borrow_and_update().clone();
        match current {
            Status::Ready(ready) => {
                if let Some(events) = ready.events.upgrade() {
                    let events = events.subscribe();
                    return Some((ready, events));
                }
            }
            Status::Failed(reason) => inbox.set(LinkState::Failed(reason)),
            Status::Starting => inbox.set(LinkState::Connecting),
        }
        status.changed().await.ok()?;
    }
}

/// The room id of a room id or alias.
async fn find(client: &Client, room: &str) -> Result<OwnedRoomId, String> {
    if let Ok(id) = RoomId::parse(room) {
        return Ok(id);
    }
    let alias = RoomAliasId::parse(room).map_err(|_| ROOM_NOT_FOUND.to_owned())?;
    client
        .resolve_room_alias(&alias)
        .await
        .map(|found| found.room_id)
        .map_err(|error| {
            tracing::warn!(%error, room, "Matrix room alias not resolved");
            ROOM_NOT_FOUND.to_owned()
        })
}

/// Joins `id`: an invite to it is accepted, otherwise the room is joined as `asked`.
async fn enter(client: &Client, id: &RoomId, asked: &str) -> Result<Room, String> {
    let joined = match client.get_room(id) {
        Some(room) if room.state() == RoomState::Joined => return Ok(room),
        Some(room) if room.state() == RoomState::Invited => room.join().await.map(|()| room),
        _ => {
            let target =
                <&RoomOrAliasId>::try_from(asked).map_err(|_| ROOM_NOT_FOUND.to_owned())?;
            client.join_room_by_id_or_alias(target, &[]).await
        }
    };
    joined.map_err(|error| {
        tracing::warn!(%error, room = asked, "Matrix room not joined");
        ROOM_NOT_FOUND.to_owned()
    })
}

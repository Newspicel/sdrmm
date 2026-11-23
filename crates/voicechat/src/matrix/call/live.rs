//! A joined call: audio both ways, members coming and going, our membership kept alive.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use futures::future::{BoxFuture, select_all};
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::broadcast::{self, error::RecvError};
use tokio::time::{MissedTickBehavior, interval_at};

use super::SyncEvent;
use super::homeserver::Homeserver;
use super::keep::{Keeper, Kept};
use super::media::Media;
use super::people::{Change, People};
use super::run::{Call, Presence, Stop, now_ms, take_key};
use super::wire::{Dial, Role, Wire, WireEvent};
use crate::link::{Backoff, Ends, Inbox, Outgoing};
use crate::matrix::e2ee::keys::{EncryptionKeys, Outgoing as Key};
use crate::matrix::livekit::peer::OpusOut;
use crate::matrix::membership::{Focus, Member, StateEntry, active_focus, identities};
use crate::opus::Framer;
use crate::{FRAME, Heard, LinkState, RATE_HZ};

/// How often the delayed leave restarts, as js-sdk `MembershipManager` does.
const RESTART_EVERY: Duration = Duration::from_secs(5);

/// What a chore hands back to the loop.
enum Done {
    Nothing,
    Kept(Kept),
    Name(String, String),
    Wire(String, Result<Box<dyn Wire>, String>),
    /// A focus to dial again once its backoff is over.
    Redial(String),
    /// Members a media key did not reach yet.
    Missed {
        to: Vec<(String, String)>,
        content: Box<EncryptionKeys>,
        backoff: Backoff,
    },
}

/// RTP time of our frames: one frame per frame sent, and over silence as much as passed.
struct Clock {
    start: Instant,
    next: u64,
}

impl Clock {
    fn stamp(&mut self) -> u32 {
        let elapsed = u64::try_from(self.start.elapsed().as_micros()).unwrap_or(u64::MAX);
        let wall = elapsed.saturating_mul(u64::from(RATE_HZ)) / 1_000_000;
        let stamp = self.next.max(wall);
        self.next = stamp + FRAME as u64;
        stamp as u32
    }
}

pub(super) struct Live<'a, H, D> {
    hs: &'a H,
    dial: &'a D,
    ours: Focus,
    own: Box<dyn Wire>,
    /// Listening wires to the foci other members publish on, by service URL.
    others: HashMap<String, Box<dyn Wire>>,
    dialing: HashSet<String>,
    /// Backoffs of foci that failed, cleared once one is heard.
    redials: HashMap<String, Backoff>,
    /// Homeserver and dialing work, run beside the audio so neither waits on it.
    chores: FuturesUnordered<BoxFuture<'a, Done>>,
    keeper: Keeper,
    clock: Clock,
    framer: Framer,
    drops: u64,
}

type Next = (Option<String>, Result<WireEvent, String>);

async fn next_event(own: &mut Box<dyn Wire>, others: &mut HashMap<String, Box<dyn Wire>>) -> Next {
    let mut all: Vec<BoxFuture<'_, Next>> = Vec::with_capacity(1 + others.len());
    all.push(Box::pin(async { (None, own.next().await) }));
    for (url, wire) in others.iter_mut() {
        all.push(Box::pin(
            async move { (Some(url.clone()), wire.next().await) },
        ));
    }
    select_all(all).await.0
}

async fn listen<D: Dial>(dial: &D, focus: Focus) -> Result<Box<dyn Wire>, String> {
    let grant = dial.grant(&focus).await?;
    dial.connect(&grant, Role::Listen).await
}

impl<'a, H: Homeserver, D: Dial> Live<'a, H, D> {
    pub(super) fn new(
        hs: &'a H,
        dial: &'a D,
        ours: Focus,
        own: Box<dyn Wire>,
        entry: StateEntry,
    ) -> Self {
        Self {
            hs,
            dial,
            ours,
            own,
            others: HashMap::new(),
            dialing: HashSet::new(),
            redials: HashMap::new(),
            chores: FuturesUnordered::new(),
            keeper: Keeper::new(entry),
            clock: Clock {
                start: Instant::now(),
                next: 0,
            },
            framer: Framer::default(),
            drops: 0,
        }
    }

    pub(super) async fn run(
        &mut self,
        call: &mut Call,
        presence: &mut Presence,
        events: &mut broadcast::Receiver<SyncEvent>,
        ends: &mut Ends,
    ) -> Stop {
        let everyone = call.people.members().to_vec();
        self.welcome(&call.people, &mut call.media, &everyone);
        self.follow_foci(call.people.members());
        let mut tick = interval_at((Instant::now() + RESTART_EVERY).into(), RESTART_EVERY);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                outgoing = ends.outbox.next() => self.say(&mut call.media, outgoing),
                (from, event) = next_event(&mut self.own, &mut self.others) => {
                    if let Some(stop) = self.on_wire(call, &ends.inbox, from, event) {
                        return stop;
                    }
                }
                Some(done) = self.chores.next() => {
                    if let Some(stop) = self.on_done(call, presence, done) {
                        return stop;
                    }
                }
                event = events.recv() => {
                    if let Some(stop) = self.on_sync(call, event) {
                        return stop;
                    }
                }
                _ = tick.tick() => self.on_tick(call, presence),
                () = ends.inbox.released() => return Stop::Released,
            }
            self.count_drops(&ends.inbox);
        }
    }

    pub(super) async fn leave(mut self) {
        self.own.leave().await;
        for (_, mut wire) in self.others.drain() {
            wire.leave().await;
        }
    }

    fn say(&mut self, media: &mut Media, outgoing: Outgoing) {
        match outgoing {
            Outgoing::Frame(samples) => {
                self.framer.push(&samples);
                while let Some(frame) = self.framer.pop() {
                    self.send(media, &frame);
                }
            }
            Outgoing::Idle => {
                if let Some(frame) = self.framer.flush() {
                    self.send(media, &frame);
                }
            }
        }
    }

    fn send(&mut self, media: &mut Media, frame: &[f32]) {
        let timestamp = self.clock.stamp();
        match media.outgoing(frame, timestamp, Instant::now()) {
            Ok(payload) => self.own.publish(OpusOut { payload, timestamp }),
            Err(reason) => tracing::warn!(reason, "frame not sent"),
        }
    }

    fn on_wire(
        &mut self,
        call: &mut Call,
        inbox: &Inbox,
        from: Option<String>,
        event: Result<WireEvent, String>,
    ) -> Option<Stop> {
        if let (Some(url), Ok(WireEvent::Heard { .. })) = (&from, &event) {
            self.redials.remove(url);
        }
        match (from, event) {
            (None, Err(reason)) => return Some(Stop::Lost(reason)),
            (Some(url), Err(reason)) => {
                self.drop_other(&url);
                self.redial(url, &reason);
            }
            (_, Ok(WireEvent::Heard { identity, payload })) => {
                if let Some(pcm) = call.media.incoming(&identity, &payload) {
                    let talker = call.people.talker(&identity);
                    inbox.hear(Heard { talker, pcm });
                }
            }
            (_, Ok(WireEvent::Left(identity))) => call.media.forget(&identity),
            (None, Ok(WireEvent::Up)) => inbox.set(LinkState::Joined),
            (Some(_), Ok(WireEvent::Up)) => {}
        }
        None
    }

    fn on_sync(&mut self, call: &mut Call, event: Result<SyncEvent, RecvError>) -> Option<Stop> {
        match event {
            Ok(SyncEvent::Key {
                sender,
                device,
                content,
            }) => take_key(call, &sender, device.as_deref(), &content),
            Ok(SyncEvent::Member { room, state }) => {
                if room == call.people.room() {
                    let change = call.people.update(state, now_ms());
                    self.on_change(call, &change);
                }
            }
            Err(RecvError::Lagged(missed)) => tracing::warn!(missed, "sync events missed"),
            Err(RecvError::Closed) => return Some(Stop::Closed),
        }
        None
    }

    /// As js-sdk does: a member leaving rotates our key for everyone left, a member joining
    /// gets the key in use.
    fn on_change(&mut self, call: &mut Call, change: &Change) {
        if change.is_empty() {
            return;
        }
        for member in &change.left {
            for identity in identities(member) {
                call.media.forget(&identity);
            }
        }
        if change.left.is_empty() {
            self.welcome(&call.people, &mut call.media, &change.joined);
        } else {
            self.name(&change.joined);
            let rotated = call.media.keys().map(|keys| keys.on_left(Instant::now()));
            match rotated {
                Some(Ok(key)) => self.send_key(&call.people, &key, call.people.members()),
                Some(Err(error)) => tracing::warn!(%error, "media key not rotated"),
                None => {}
            }
        }
        self.follow_foci(call.people.members());
    }

    fn welcome(&mut self, people: &People, media: &mut Media, members: &[Member]) {
        self.name(members);
        if let Some(keys) = media.keys() {
            self.send_key(people, &keys.on_joined(), members);
        }
    }

    fn name(&mut self, members: &[Member]) {
        for member in members {
            let (hs, user) = (self.hs, member.user.clone());
            self.chores.push(Box::pin(async move {
                match hs.display_name(&user).await {
                    Some(name) => Done::Name(user, name),
                    None => Done::Nothing,
                }
            }));
        }
    }

    fn send_key(&mut self, people: &People, key: &Key, to: &[Member]) {
        if to.is_empty() {
            return;
        }
        let me = people.me();
        let content = EncryptionKeys::new(people.room(), &me.member_id(), &me.device, key);
        let targets: Vec<(String, String)> = to
            .iter()
            .map(|member| (member.user.clone(), member.device.clone()))
            .collect();
        self.deliver(targets, Box::new(content), Backoff::new(), Duration::ZERO);
    }

    /// Sends a media key after `wait`, handing back the members it did not reach.
    fn deliver(
        &mut self,
        to: Vec<(String, String)>,
        content: Box<EncryptionKeys>,
        backoff: Backoff,
        wait: Duration,
    ) {
        let hs = self.hs;
        self.chores.push(Box::pin(async move {
            tokio::time::sleep(wait).await;
            let missed = match hs.send_keys(&to, &content).await {
                Ok(missed) => missed,
                Err(reason) => {
                    tracing::warn!(reason, "media key not sent");
                    to
                }
            };
            if missed.is_empty() {
                Done::Nothing
            } else {
                Done::Missed {
                    to: missed,
                    content,
                    backoff,
                }
            }
        }));
    }

    /// Tries a missed key again for the members still in the call.
    fn resend(
        &mut self,
        members: &[Member],
        to: Vec<(String, String)>,
        content: Box<EncryptionKeys>,
        mut backoff: Backoff,
    ) {
        let left: Vec<(String, String)> = to
            .into_iter()
            .filter(|(user, device)| {
                members
                    .iter()
                    .any(|member| member.user == *user && member.device == *device)
            })
            .collect();
        if left.is_empty() {
            return;
        }
        let wait = backoff.step();
        tracing::warn!(members = left.len(), retry_in = ?wait, "media key not delivered");
        self.deliver(left, content, backoff, wait);
    }

    /// The foci we listen on: each member's own, or the active one for members naming none.
    fn wanted(&self, members: &[Member]) -> HashSet<String> {
        let active = active_focus(members, &self.ours);
        members
            .iter()
            .map(|member| member.focus.as_ref().unwrap_or(&active).service_url.clone())
            .filter(|url| *url != self.ours.service_url)
            .collect()
    }

    fn follow_foci(&mut self, members: &[Member]) {
        let wanted = self.wanted(members);
        let gone: Vec<String> = self
            .others
            .keys()
            .filter(|url| !wanted.contains(*url))
            .cloned()
            .collect();
        for url in gone {
            self.drop_other(&url);
        }
        self.redials.retain(|url, _| wanted.contains(url));
        for url in wanted {
            if self.others.contains_key(&url) || !self.dialing.insert(url.clone()) {
                continue;
            }
            let dial = self.dial;
            self.chores.push(Box::pin(async move {
                let focus = Focus {
                    service_url: url.clone(),
                };
                Done::Wire(url, listen(dial, focus).await)
            }));
        }
    }

    /// Dials a failed focus again after its backoff, so one bad focus leaves only its members
    /// unheard and only for a while.
    fn redial(&mut self, url: String, reason: &str) {
        self.dialing.insert(url.clone());
        let wait = self
            .redials
            .entry(url.clone())
            .or_insert_with(Backoff::new)
            .step();
        tracing::warn!(url, reason, retry_in = ?wait, "cannot listen on focus");
        self.chores.push(Box::pin(async move {
            tokio::time::sleep(wait).await;
            Done::Redial(url)
        }));
    }

    fn drop_other(&mut self, url: &str) {
        let Some(mut wire) = self.others.remove(url) else {
            return;
        };
        self.drops = self.drops.saturating_sub(wire.dropped());
        self.chores.push(Box::pin(async move {
            wire.leave().await;
            Done::Nothing
        }));
    }

    fn on_done(&mut self, call: &mut Call, presence: &mut Presence, done: Done) -> Option<Stop> {
        match done {
            Done::Nothing => {}
            Done::Kept(kept) => match self.keeper.done(self.hs, presence, kept) {
                Ok(next) => self.keep(next),
                Err(stop) => return Some(stop),
            },
            Done::Name(user, name) => call.people.named(user, name),
            Done::Wire(url, Ok(wire)) => {
                self.dialing.remove(&url);
                self.others.insert(url, wire);
                self.follow_foci(call.people.members());
            }
            Done::Wire(url, Err(reason)) => self.redial(url, &reason),
            Done::Redial(url) => {
                self.dialing.remove(&url);
                self.follow_foci(call.people.members());
            }
            Done::Missed {
                to,
                content,
                backoff,
            } => self.resend(call.people.members(), to, content, backoff),
        }
        None
    }

    fn keep(&mut self, requests: impl IntoIterator<Item = BoxFuture<'a, Kept>>) {
        for request in requests {
            self.chores
                .push(Box::pin(async move { Done::Kept(request.await) }));
        }
    }

    fn on_tick(&mut self, call: &mut Call, presence: &Presence) {
        let due = self.keeper.tick(self.hs, presence);
        self.keep(due);
        let change = call.people.recount(now_ms());
        self.on_change(call, &change);
    }

    fn count_drops(&mut self, inbox: &Inbox) {
        let total =
            self.own.dropped() + self.others.values().map(|wire| wire.dropped()).sum::<u64>();
        for _ in self.drops..total {
            inbox.lose("heard queue full");
        }
        self.drops = total;
    }
}

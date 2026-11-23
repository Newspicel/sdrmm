//! Voice chat nodes: each joins one channel of a chat account and trades audio with the patch.

use std::{
    collections::{BTreeSet, HashMap, HashSet, hash_map::Entry},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use sdrmm_dsp::audio::mix::lane_of;
use sdrmm_engine::Engine;
use sdrmm_voicechat::{FRAME, Hold, Link, LinkState, RATE_HZ};
use sdrmm_wire::{
    AUDIO_PORT, AudioRoute, ChatAccount, ChatService, NodeBody, NodeProblem, PatchGraph,
    ProblemSource, ResolvedPatch, ServerEvent, VoiceChatNode, audio::AUDIO_RATE_HZ,
};
use tokio::{
    runtime::Handle,
    sync::{mpsc, watch},
    task::JoinHandle,
};

use crate::{AppState, resolved::Resolved};

mod gate;
mod hub;
mod status;
mod to_air;
mod to_chat;

use hub::{Links, Sessions};
use status::Meters;
use to_air::{Air, Said};
use to_chat::Feed;

const _: () = assert!(
    AUDIO_RATE_HZ == RATE_HZ,
    "channel audio must be at the chat's rate"
);

/// How often nodes pick up a channel that started late, and how often status goes out.
const RECHECK: Duration = Duration::from_secs(1);
/// How far a mixed source may fall behind before the mix stops waiting for it: three frames.
const MIX_LAG: usize = 3 * FRAME;
/// Channel blocks queued toward one node's mix before a wire waits.
const FEED_QUEUE: usize = 64;
/// How long a link that gave up stays shown before the node tries again.
const RETRY_FAILED: Duration = Duration::from_secs(15);
const CLASH: &str = "Another node is in this channel";

/// What a node's audio tasks follow while it stays in its channel.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Tuning {
    node: VoiceChatNode,
    /// The out port feeds a running encoder.
    keying: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct Want {
    account: ChatAccount,
    channel: String,
    routes: HashSet<AudioRoute>,
    tuning: Tuning,
}

struct ToChatRun {
    feeds: mpsc::Sender<Feed>,
    task: JoinHandle<()>,
    wires: HashMap<AudioRoute, JoinHandle<()>>,
}

impl Drop for ToChatRun {
    fn drop(&mut self) {
        self.task.abort();
        for wire in self.wires.values() {
            wire.abort();
        }
    }
}

/// A node in its voice channel.
struct Joined {
    account: String,
    channel: String,
    send: mpsc::Sender<Vec<f32>>,
    link: watch::Receiver<LinkState>,
    drops: Arc<AtomicU64>,
    seen_drops: u64,
    tuning: watch::Sender<Tuning>,
    meters: Arc<Meters>,
    to_air: JoinHandle<()>,
    to_chat: Option<ToChatRun>,
    watcher: JoinHandle<()>,
    ended_at: Option<tokio::time::Instant>,
    /// Dropped with the node, so its seat is free for the next join in the same pass.
    _hold: Hold,
}

impl Drop for Joined {
    fn drop(&mut self) {
        self.to_air.abort();
        self.watcher.abort();
    }
}

/// What a settle step needs from the server, so tests can stand in for it.
struct Ctx<'a> {
    engine: &'a Arc<Engine>,
    runtime: &'a Handle,
    sync_fx: &'a dyn Fn(),
    keying: &'a dyn Fn(&str) -> bool,
    air: &'a dyn Fn(&str) -> Air,
}

/// Every voice chat node of the active patch.
struct VoiceChats {
    links: Arc<dyn Links>,
    accounts: Vec<ChatAccount>,
    accounts_changed: bool,
    used: BTreeSet<String>,
    joined: HashMap<String, Joined>,
}

pub(crate) fn follow(state: &AppState) {
    let Ok(runtime) = Handle::try_current() else {
        tracing::warn!("no runtime in context: voice chat will not follow the patch");
        return;
    };
    let chats = Arc::new(Mutex::new(VoiceChats::new(Arc::new(Sessions::new(
        state.store.clone(),
    )))));
    let mut accounts = state.store.subscribe_chat_accounts();
    accounts.mark_changed();
    let mut armed = state.encoders.subscribe_changes();
    let held = chats.clone();
    let handle = runtime.clone();
    crate::resolved::follow(state, "voice chat", move |state| {
        lock(&held).reconcile(state, &handle, None);
    });
    let state = state.clone();
    runtime.clone().spawn(async move {
        let mut tick = tokio::time::interval(RECHECK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                changed = armed.changed() => if changed.is_err() {
                    tracing::error!("voice chat lost the transmitters it follows");
                    return;
                },
            }
            let edited = accounts.has_changed().unwrap_or(false);
            accounts.mark_unchanged();
            let (state, chats, runtime) = (state.clone(), chats.clone(), runtime.clone());
            let step = move || {
                let accounts = edited.then(|| state.store.chat_accounts());
                lock(&chats).reconcile(&state, &runtime, accounts);
            };
            if let Err(error) = tokio::task::spawn_blocking(step).await {
                tracing::error!(%error, "voice chat stopped following the patch");
                return;
            }
        }
    });
}

fn lock(chats: &Mutex<VoiceChats>) -> MutexGuard<'_, VoiceChats> {
    chats.lock().unwrap_or_else(PoisonError::into_inner)
}

impl VoiceChats {
    fn new(links: Arc<dyn Links>) -> Self {
        Self {
            links,
            accounts: Vec::new(),
            accounts_changed: false,
            used: BTreeSet::new(),
            joined: HashMap::new(),
        }
    }

    fn reconcile(
        &mut self,
        state: &AppState,
        runtime: &Handle,
        accounts: Option<Result<Vec<ChatAccount>, crate::store::StoreError>>,
    ) {
        match accounts {
            Some(Ok(accounts)) => self.set_accounts(accounts),
            Some(Err(error)) => tracing::error!(%error, "could not read the chat accounts"),
            None => {}
        }
        let current = state.resolved.current();
        let engine = &state.engine;
        let keying = |node: &str| crate::encoders::feeds_encoder(engine, &current, node);
        let air = |node: &str| -> Air {
            let (state, node) = (state.clone(), node.to_owned());
            let lane = lane_of(&node);
            Arc::new(move |pcm: &[f32]| crate::encoders::speak_as(&state, &node, lane, pcm))
        };
        let ctx = Ctx {
            engine,
            runtime,
            sync_fx: &|| crate::audio_fx::sync(engine, &state.resolved),
            keying: &keying,
            air: &air,
        };
        let problems = self.settle(&ctx, &current);
        state
            .problems
            .replace(engine, ProblemSource::VoiceChat, &problems);
        for status in self.statuses(&current.graph, &problems) {
            engine.emit_event(ServerEvent::VoiceChatStatus(status));
        }
    }

    fn set_accounts(&mut self, accounts: Vec<ChatAccount>) {
        self.accounts = accounts;
        self.accounts_changed = true;
    }

    fn settle(&mut self, ctx: &Ctx<'_>, current: &Resolved) -> Vec<NodeProblem> {
        let (wanted, mut problems) = self.wanted(&current.graph, &current.patch, ctx.keying);
        self.joined.retain(|node, joined| {
            wanted.get(node).is_some_and(|want| joined.serves(want)) && !joined.spent()
        });
        self.settle_sessions(&wanted);
        let mut nodes: Vec<&String> = wanted.keys().collect();
        nodes.sort();
        for node in nodes {
            match self.run(ctx, node, &wanted[node]) {
                Ok(joined) => problems.extend(joined.fault().map(|text| problem(node, text))),
                Err(text) => problems.push(problem(node, text)),
            }
        }
        problems
    }

    /// The nodes that should be in a channel now. A second node on a channel another node
    /// holds is a fault; the node already in it keeps it.
    fn wanted(
        &self,
        graph: &PatchGraph,
        patch: &ResolvedPatch,
        keying: &dyn Fn(&str) -> bool,
    ) -> (HashMap<String, Want>, Vec<NodeProblem>) {
        let mut nodes: Vec<(&str, &VoiceChatNode)> = graph
            .nodes
            .iter()
            .filter_map(|node| match &node.body {
                NodeBody::VoiceChat(chat) => Some((node.id.as_str(), chat)),
                _ => None,
            })
            .collect();
        nodes.sort_by_key(|(id, _)| !self.joined.contains_key(*id));
        let mut taken = HashSet::new();
        let mut wanted = HashMap::new();
        let mut problems = Vec::new();
        for (id, chat) in nodes {
            let Some(want) = self.want(graph, patch, id, chat, keying) else {
                continue;
            };
            if taken.insert((want.account.id.clone(), want.channel.clone())) {
                wanted.insert(id.to_owned(), want);
            } else {
                problems.push(problem(id, CLASH.to_owned()));
            }
        }
        (wanted, problems)
    }

    /// None while the node has no known account, or no channel on a service without a
    /// default one: it stays idle. A node with no
    /// wire stays in its channel, so wiring it later does not rejoin.
    fn want(
        &self,
        graph: &PatchGraph,
        patch: &ResolvedPatch,
        id: &str,
        chat: &VoiceChatNode,
        keying: &dyn Fn(&str) -> bool,
    ) -> Option<Want> {
        let account = self
            .accounts
            .iter()
            .find(|account| account.id == chat.account)?;
        let channel = chat.channel.trim();
        if channel.is_empty() && !has_default_channel(&account.service) {
            return None;
        }
        let sends_out = !patch.reaches(id, AUDIO_PORT).is_empty();
        Some(Want {
            account: account.clone(),
            channel: channel.to_owned(),
            routes: routes(graph, patch, id),
            tuning: Tuning {
                node: chat.clone(),
                keying: sends_out && keying(id),
            },
        })
    }

    /// Hands the hub the accounts in use when they or their settings changed; links on a
    /// session it closed are gone and join again.
    fn settle_sessions(&mut self, wanted: &HashMap<String, Want>) {
        let used: BTreeSet<String> = wanted
            .values()
            .map(|want| want.account.id.clone())
            .collect();
        if !self.accounts_changed && used == self.used {
            return;
        }
        self.accounts_changed = false;
        let accounts: Vec<ChatAccount> = self
            .accounts
            .iter()
            .filter(|account| used.contains(&account.id))
            .cloned()
            .collect();
        self.used = used;
        let closed = self.links.settle(&accounts);
        self.joined
            .retain(|_, joined| !closed.contains(&joined.account));
    }

    fn run(&mut self, ctx: &Ctx<'_>, node: &str, want: &Want) -> Result<&mut Joined, String> {
        let joined = match self.joined.entry(node.to_owned()) {
            Entry::Occupied(held) => held.into_mut(),
            Entry::Vacant(free) => {
                let link = self.links.join(&want.account, &want.channel)?;
                free.insert(Joined::open(ctx, node, want, link))
            }
        };
        joined.tuning.send_if_modified(|tuning| {
            let changed = *tuning != want.tuning;
            tuning.clone_from(&want.tuning);
            changed
        });
        joined.wire(ctx, &want.routes)?;
        joined.note_drops(node);
        Ok(joined)
    }

    fn statuses(
        &self,
        graph: &PatchGraph,
        problems: &[NodeProblem],
    ) -> Vec<sdrmm_wire::VoiceChatStatus> {
        graph
            .nodes
            .iter()
            .filter(|node| matches!(node.body, NodeBody::VoiceChat(_)))
            .map(|node| {
                if let Some(joined) = self.joined.get(&node.id) {
                    return joined.status(&node.id);
                }
                let fault = problems.iter().find(|problem| problem.node == node.id);
                status::idle(&node.id, fault.map(|problem| problem.text.clone()))
            })
            .collect()
    }
}

impl Joined {
    fn open(ctx: &Ctx<'_>, node: &str, want: &Want, link: Link) -> Self {
        let meters = Arc::new(Meters::default());
        let (tuning, tuned) = watch::channel(want.tuning.clone());
        let Link {
            send,
            heard,
            state,
            drops,
            hold,
        } = link;
        let to_air = ctx.runtime.spawn(to_air::run(
            heard,
            tuned,
            meters.clone(),
            ((ctx.air)(node), said(ctx.engine, node)),
        ));
        let watcher = ctx.runtime.spawn(status::watch(
            node.to_owned(),
            ctx.engine.clone(),
            state.clone(),
            meters.clone(),
        ));
        Self {
            account: want.account.id.clone(),
            channel: want.channel.clone(),
            drops,
            send,
            link: state,
            seen_drops: 0,
            tuning,
            meters,
            to_air,
            to_chat: None,
            watcher,
            ended_at: None,
            _hold: hold,
        }
    }

    fn serves(&self, want: &Want) -> bool {
        self.account == want.account.id && self.channel == want.channel
    }

    /// The link is over: its session closed, or it gave up a while ago.
    fn spent(&mut self) -> bool {
        if !self.to_air.is_finished() {
            return false;
        }
        if !matches!(*self.link.borrow(), LinkState::Failed(_)) {
            return true;
        }
        let ended = *self.ended_at.get_or_insert_with(tokio::time::Instant::now);
        ended.elapsed() >= RETRY_FAILED
    }

    fn fault(&self) -> Option<String> {
        match &*self.link.borrow() {
            LinkState::Failed(reason) => Some(reason.clone()),
            LinkState::Connecting | LinkState::Joined => None,
        }
    }

    fn status(&self, node: &str) -> sdrmm_wire::VoiceChatStatus {
        status::joined(
            node,
            &self.link.borrow(),
            self.to_air.is_finished(),
            &self.meters,
        )
    }

    fn note_drops(&mut self, node: &str) {
        let dropped = self.drops.load(Ordering::Relaxed);
        if dropped > self.seen_drops {
            tracing::warn!(
                node,
                lost = dropped - self.seen_drops,
                total = dropped,
                "voice chat lost frames it heard"
            );
            self.seen_drops = dropped;
        }
    }

    /// Runs the chat direction while wired channels carry audio, one wire per route.
    fn wire(&mut self, ctx: &Ctx<'_>, routes: &HashSet<AudioRoute>) -> Result<(), String> {
        if routes.is_empty() {
            if self.to_chat.take().is_some() {
                self.meters.chat(None, false);
            }
            return Ok(());
        }
        let run = self.to_chat.get_or_insert_with(|| {
            let (feeds, fed) = mpsc::channel(FEED_QUEUE);
            let task = ctx.runtime.spawn(to_chat::run(
                fed,
                self.send.clone(),
                self.tuning.subscribe(),
                self.meters.clone(),
            ));
            ToChatRun {
                feeds,
                task,
                wires: HashMap::new(),
            }
        });
        run.wires.retain(|route, wire| {
            let kept = routes.contains(route) && !wire.is_finished();
            if !kept {
                wire.abort();
                if run
                    .feeds
                    .try_send(Feed::Gone(crate::encoders::route_lane(route)))
                    .is_err()
                {
                    tracing::debug!(?route, "voice chat mix forgets the wire on its own");
                }
            }
            kept
        });
        for route in routes {
            if run.wires.contains_key(route) {
                continue;
            }
            let Some(pcm) = subscribe(ctx, route)? else {
                continue;
            };
            let lane = crate::encoders::route_lane(route);
            let wire = ctx
                .runtime
                .spawn(to_chat::forward(pcm, lane, run.feeds.clone()));
            run.wires.insert(route.clone(), wire);
        }
        Ok(())
    }
}

/// Speaks what the chat says as the node's voice, for the wires on its audio out.
fn said(engine: &Arc<Engine>, node: &str) -> Said {
    let (engine, node) = (engine.clone(), node.to_owned());
    Arc::new(move |pcm: &[f32]| engine.speak_voice(&node, pcm))
}

/// The wired audio, or None while its channel is not running yet.
fn subscribe(
    ctx: &Ctx<'_>,
    route: &AudioRoute,
) -> Result<Option<tokio::sync::broadcast::Receiver<sdrmm_engine::PcmBlock>>, String> {
    if !route.fx.is_empty() {
        (ctx.sync_fx)();
    }
    match ctx.engine.subscribe_route_pcm(route) {
        Ok(pcm) => Ok(Some(pcm)),
        Err(error) if error.is_not_found() => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// A `TeamSpeak` server seats a client that names no channel in its default one; Discord has
/// no such channel.
fn has_default_channel(service: &ChatService) -> bool {
    matches!(service, ChatService::Teamspeak { .. })
}

fn routes(graph: &PatchGraph, patch: &ResolvedPatch, node: &str) -> HashSet<AudioRoute> {
    patch
        .feeds(node, AUDIO_PORT)
        .iter()
        .flat_map(|feed| feed.audio_routes(graph))
        .collect()
}

fn problem(node: &str, text: String) -> NodeProblem {
    NodeProblem {
        node: node.to_owned(),
        source: ProblemSource::VoiceChat,
        text,
    }
}

#[cfg(test)]
mod tests;

//! Discord voice: one gateway shard per bot, songbird for the voice connection and DAVE.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex, PoisonError, mpsc as std_mpsc};
use std::time::Duration;

use songbird::driver::{Channels, DecodeConfig, DecodeMode, SampleRate};
use songbird::events::context_data::{DisconnectData, DisconnectReason, VoiceTick};
use songbird::events::{CoreEvent, Event, EventContext, EventHandler};
use songbird::input::RawAdapter;
use songbird::input::core::io::MediaSource;
use songbird::shards::TwilightMap;
use songbird::tracks::TrackHandle;
use songbird::{Call, Config, Songbird};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};
use twilight_gateway::{
    CloseFrame, Event as GatewayEvent, EventTypeFlags, Intents, Shard, ShardId, StreamExt as _,
};
use twilight_http::error::ErrorType;
use twilight_model::channel::ChannelType;
use twilight_model::gateway::payload::incoming::GuildCreate;
use twilight_model::guild::Member;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker};

use crate::link::{self, Backoff, Ear, Ends, Inbox, Seat};
use crate::{FRAME, Heard, Link, LinkState, RATE_HZ, Talker, VoiceChatError, lock};

const WANTED: EventTypeFlags = EventTypeFlags::READY
    .union(EventTypeFlags::GUILD_CREATE)
    .union(EventTypeFlags::VOICE_STATE_UPDATE)
    .union(EventTypeFlags::VOICE_SERVER_UPDATE);
/// How long a closing session waits for Discord to hear it leave.
const GOODBYE: Duration = Duration::from_secs(2);
/// How long a released link keeps the bot in its channel for a successor in the same server
/// to take the seat, so a channel change moves the bot instead of leaving and joining again.
const HANDOVER: Duration = Duration::from_secs(1);
/// Frames the voice track may fall behind `send` before the oldest drop, 100 ms.
const BACKLOG: usize = 5;

pub(crate) struct Discord {
    token: String,
    /// Followed in every server the bot sits in, renaming it there without leaving.
    nickname: watch::Sender<String>,
    gateway: Mutex<Option<Arc<Gateway>>>,
    alive: mpsc::Sender<()>,
    _alive: mpsc::Receiver<()>,
}

impl Discord {
    pub(crate) fn new(token: String, nickname: String) -> Self {
        let (alive, keep) = mpsc::channel(1);
        Self {
            token,
            nickname: watch::Sender::new(nickname),
            gateway: Mutex::new(None),
            alive,
            _alive: keep,
        }
    }

    pub(crate) fn join(&self, runtime: &Handle, channel: &str) -> Result<Link, VoiceChatError> {
        let channel = channel.trim();
        if channel.is_empty() {
            return Err(VoiceChatError::NoChannel);
        }
        let id = channel
            .parse()
            .ok()
            .and_then(Id::<ChannelMarker>::new_checked)
            .ok_or_else(|| VoiceChatError::BadChannel(channel.to_owned()))?;
        let gateway = self.gateway(runtime);
        if let Some(taken) = gateway.taken(id) {
            return Err(VoiceChatError::Clash(taken));
        }
        let (link, ends) = link::link();
        let names = self.nickname.subscribe();
        runtime.spawn(run(gateway, id, ends, names, self.alive.clone()));
        Ok(link)
    }

    pub(crate) fn rename(&self, nickname: String) {
        self.nickname.send_replace(nickname);
    }

    /// The bot's gateway, started by the first join.
    fn gateway(&self, runtime: &Handle) -> Arc<Gateway> {
        let mut slot = lock(&self.gateway);
        if let Some(gateway) = slot.as_ref() {
            return Arc::clone(gateway);
        }
        let gateway = Gateway::start(runtime, &self.token, &self.alive);
        *slot = Some(Arc::clone(&gateway));
        gateway
    }
}

enum Ended {
    Released,
    Failed(String),
    /// Another link holds the server's voice seat; retrying cannot help.
    Clash(String),
}

async fn run(
    gateway: Arc<Gateway>,
    channel: Id<ChannelMarker>,
    mut ends: Ends,
    names: watch::Receiver<String>,
    alive: mpsc::Sender<()>,
) {
    let mut backoff = Backoff::new();
    loop {
        let ended = tokio::select! {
            ended = attempt(&gateway, channel, &mut ends, &names, &mut backoff) => ended,
            () = alive.closed() => return,
        };
        match ended {
            Ended::Released => return,
            Ended::Clash(reason) => return ends.inbox.set(LinkState::Failed(reason)),
            Ended::Failed(reason) => ends.inbox.set(LinkState::Failed(reason)),
        }
        tokio::select! {
            () = tokio::time::sleep(backoff.step()) => {}
            () = ends.inbox.released() => return,
            () = alive.closed() => return,
            never = ends.outbox.discard() => match never {},
        }
    }
}

/// One join, from finding the server to leaving.
async fn attempt(
    gateway: &Arc<Gateway>,
    channel: Id<ChannelMarker>,
    ends: &mut Ends,
    names: &watch::Receiver<String>,
    backoff: &mut Backoff,
) -> Ended {
    let mut voice = gateway.voice();
    let found = tokio::select! {
        found = server(gateway, channel, &mut voice) => found,
        () = ends.inbox.released() => return Ended::Released,
        never = ends.outbox.discard() => match never {},
    };
    let (songbird, guild) = match found {
        Ok(found) => found,
        Err(reason) => return Ended::Failed(reason),
    };
    if let Some(taken) = gateway.seats.claim(guild, channel, &ends.inbox) {
        return Ended::Clash(format!("already in {taken}"));
    }
    let joined = tokio::select! {
        joined = songbird.join(guild, channel) => joined,
        never = ends.outbox.discard() => match never {},
    };
    let call = match joined {
        Ok(call) => call,
        Err(error) => return Ended::Failed(format!("join: {error}")),
    };
    let (news_tx, mut news) = mpsc::unbounded_channel();
    let listener = Listener::new(Arc::clone(gateway), ends.inbox.ear(), news_tx);
    let (frames, track) = equip(&call, listener).await;
    ends.outbox.clear();
    ends.inbox.set(LinkState::Joined);
    backoff.reset();
    let naming = tokio::spawn(keep_named(Arc::clone(gateway), guild, names.clone()));
    let ended = stream(&track, &frames, &mut news, &mut voice, ends).await;
    naming.abort();
    if leaves(&gateway.seats, guild, &ends.inbox, &ended).await
        && let Err(error) = songbird.remove(guild).await
    {
        tracing::debug!(%error, "Discord leave");
    }
    ended
}

/// Shows the account's nickname in `guild` while a link sits there, following renames. An
/// empty one leaves the bot's name alone, until a rename to it clears a nickname set before.
async fn keep_named(
    gateway: Arc<Gateway>,
    guild: Id<GuildMarker>,
    mut names: watch::Receiver<String>,
) {
    let first = names.borrow_and_update().clone();
    if !first.is_empty() {
        gateway.nickname(guild, &first).await;
    }
    while names.changed().await.is_ok() {
        let nickname = names.borrow_and_update().clone();
        gateway.nickname(guild, &nickname).await;
    }
}

/// Whether an ended link takes the bot out of the server's voice. A released one first waits
/// [`HANDOVER`] for a successor; one that lost its seat never leaves.
async fn leaves(seats: &Seats, guild: Id<GuildMarker>, inbox: &Inbox, ended: &Ended) -> bool {
    if matches!(ended, Ended::Released) {
        tokio::time::sleep(HANDOVER).await;
    }
    seats.seated(guild, inbox)
}

/// The live voice manager and the server `channel` belongs to.
async fn server(
    gateway: &Gateway,
    channel: Id<ChannelMarker>,
    voice: &mut watch::Receiver<Voice>,
) -> Result<(Arc<Songbird>, Id<GuildMarker>), String> {
    let songbird = ready(voice).await?;
    let guild = gateway.guild_of(channel).await?;
    Ok((songbird, guild))
}

async fn ready(voice: &mut watch::Receiver<Voice>) -> Result<Arc<Songbird>, String> {
    let state = voice
        .wait_for(|state| !matches!(state, Voice::Starting))
        .await
        .map_err(|_| "Discord stopped".to_owned())?;
    match &*state {
        Voice::Ready(songbird) => Ok(Arc::clone(songbird)),
        Voice::Failed(reason) => Err(reason.clone()),
        Voice::Starting => Err("Discord starting".to_owned()),
    }
}

/// Hooks the listener onto the call and starts the paused track that carries `send`.
async fn equip(
    call: &Arc<tokio::sync::Mutex<Call>>,
    listener: Listener,
) -> (std::sync::mpsc::Sender<Vec<f32>>, TrackHandle) {
    let mut call = call.lock().await;
    call.remove_all_global_events();
    for event in [
        CoreEvent::SpeakingStateUpdate,
        CoreEvent::VoiceTick,
        CoreEvent::DriverConnect,
        CoreEvent::DriverReconnect,
        CoreEvent::DriverDisconnect,
    ] {
        call.add_global_event(event.into(), listener.clone());
    }
    let (frames, feed) = feed();
    let track = call.play_input(RawAdapter::new(feed, RATE_HZ, 1).into());
    if let Err(error) = track.pause() {
        tracing::debug!(%error, "Discord track pause");
    }
    (frames, track)
}

/// Plays `send` into the call while frames come and pauses between spurts, so the bot only
/// shows as speaking while it is.
async fn stream(
    track: &TrackHandle,
    frames: &std::sync::mpsc::Sender<Vec<f32>>,
    news: &mut mpsc::UnboundedReceiver<Driver>,
    voice: &mut watch::Receiver<Voice>,
    ends: &mut Ends,
) -> Ended {
    let mut playing = false;
    loop {
        tokio::select! {
            outgoing = ends.outbox.next() => match outgoing {
                link::Outgoing::Frame(frame) => {
                    let resumed = playing || track.play().is_ok();
                    if !resumed || frames.send(frame).is_err() {
                        return Ended::Failed("voice track ended".into());
                    }
                    playing = true;
                }
                link::Outgoing::Idle => {
                    if playing && track.pause().is_err() {
                        return Ended::Failed("voice track ended".into());
                    }
                    playing = false;
                }
            },
            news = news.recv() => match news {
                Some(Driver::Up) => ends.inbox.set(LinkState::Joined),
                Some(Driver::Down(reason)) => return Ended::Failed(reason),
                None => return Ended::Failed("voice ended".into()),
            },
            _ = voice.changed() => return Ended::Failed("Discord reconnecting".into()),
            () = ends.inbox.released() => return Ended::Released,
        }
    }
}

/// What the voice driver reports to its link.
enum Driver {
    Up,
    Down(String),
}

/// Turns songbird's call events into heard frames and driver news.
#[derive(Clone)]
struct Listener(Arc<Listening>);

struct Listening {
    gateway: Arc<Gateway>,
    users: Mutex<HashMap<u32, u64>>,
    ear: Ear,
    news: mpsc::UnboundedSender<Driver>,
}

impl Listener {
    fn new(gateway: Arc<Gateway>, ear: Ear, news: mpsc::UnboundedSender<Driver>) -> Self {
        Self(Arc::new(Listening {
            gateway,
            users: Mutex::new(HashMap::new()),
            ear,
            news,
        }))
    }

    fn hear(&self, tick: &VoiceTick) {
        let users = lock(&self.0.users);
        for (ssrc, data) in &tick.speaking {
            let Some(pcm) = data.decoded_voice.as_ref().filter(|pcm| !pcm.is_empty()) else {
                continue;
            };
            self.0.ear.hear(Heard {
                talker: users
                    .get(ssrc)
                    .map_or_else(|| unmapped(*ssrc), |user| self.0.gateway.talker(*user)),
                pcm: pcm
                    .iter()
                    .map(|&sample| f32::from(sample) / 32_768.0)
                    .collect(),
            });
        }
    }
}

#[async_trait::async_trait]
impl EventHandler for Listener {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        match ctx {
            EventContext::SpeakingStateUpdate(speaking) => {
                if let Some(user) = speaking.user_id {
                    lock(&self.0.users).insert(speaking.ssrc, user.0);
                }
            }
            EventContext::VoiceTick(tick) => self.hear(tick),
            EventContext::DriverConnect(_) | EventContext::DriverReconnect(_) => {
                let _ = self.0.news.send(Driver::Up);
            }
            EventContext::DriverDisconnect(data) => {
                if let Some(reason) = dropped(data) {
                    let _ = self.0.news.send(Driver::Down(reason));
                }
            }
            _ => {}
        }
        None
    }
}

/// Why the voice connection dropped, or `None` when we left on purpose.
fn dropped(data: &DisconnectData<'_>) -> Option<String> {
    match data.reason {
        Some(DisconnectReason::Requested) => None,
        Some(DisconnectReason::TimedOut) => Some("voice timed out".into()),
        Some(DisconnectReason::WsClosed(Some(code))) => Some(format!("voice closed: {code:?}")),
        _ => Some("voice dropped".into()),
    }
}

/// The bot's gateway connection, shared by every link of one session.
struct Gateway {
    http: twilight_http::Client,
    voice: watch::Receiver<Voice>,
    names: Mutex<HashMap<u64, String>>,
    guilds: Mutex<HashMap<Id<ChannelMarker>, Id<GuildMarker>>>,
    seats: Seats,
}

#[derive(Clone)]
enum Voice {
    Starting,
    Ready(Arc<Songbird>),
    Failed(String),
}

impl Gateway {
    fn start(runtime: &Handle, token: &str, alive: &mpsc::Sender<()>) -> Arc<Self> {
        install_tls_provider();
        let (voice_tx, voice) = watch::channel(Voice::Starting);
        let gateway = Arc::new(Self {
            http: twilight_http::Client::new(token.to_owned()),
            voice,
            names: Mutex::new(HashMap::new()),
            guilds: Mutex::new(HashMap::new()),
            seats: Seats::default(),
        });
        runtime.spawn(run_gateway(
            token.to_owned(),
            Arc::clone(&gateway),
            voice_tx,
            alive.clone(),
        ));
        gateway
    }

    fn voice(&self) -> watch::Receiver<Voice> {
        self.voice.clone()
    }

    /// The channel a live link holds in this channel's server, when the server is known.
    fn taken(&self, channel: Id<ChannelMarker>) -> Option<String> {
        let guild = lock(&self.guilds).get(&channel).copied()?;
        self.seats.taken(guild)
    }

    async fn guild_of(&self, channel: Id<ChannelMarker>) -> Result<Id<GuildMarker>, String> {
        let known = lock(&self.guilds).get(&channel).copied();
        if let Some(guild) = known {
            return Ok(guild);
        }
        let found = self
            .http
            .channel(channel)
            .await
            .map_err(|error| lookup_failed(&error))?
            .model()
            .await
            .map_err(|error| format!("Discord: {error}"))?;
        let voice = matches!(
            found.kind,
            ChannelType::GuildVoice | ChannelType::GuildStageVoice
        );
        let guild = found
            .guild_id
            .filter(|_| voice)
            .ok_or_else(|| "not a voice channel".to_owned())?;
        lock(&self.guilds).insert(channel, guild);
        Ok(guild)
    }

    fn talker(&self, user: u64) -> Talker {
        talker(&lock(&self.names), user)
    }

    /// Sets the bot's nickname in `guild`, an empty one clearing it. A refusal, such as a
    /// missing Change Nickname permission, keeps the old name and the link.
    async fn nickname(&self, guild: Id<GuildMarker>, nickname: &str) {
        let nick = (!nickname.is_empty()).then_some(nickname);
        if let Err(error) = self.http.update_current_member(guild).nick(nick).await {
            tracing::warn!(%error, %guild, "Discord nickname not set");
        }
    }

    fn learn_guild(&self, guild: &GuildCreate) {
        let GuildCreate::Available(guild) = guild else {
            return;
        };
        lock(&self.guilds).extend(guild.channels.iter().map(|channel| (channel.id, guild.id)));
        let mut names = lock(&self.names);
        let members = guild.members.iter().chain(
            guild
                .voice_states
                .iter()
                .filter_map(|state| state.member.as_ref()),
        );
        for member in members {
            names.insert(member.user.id.get(), name_of(member));
        }
    }

    fn learn_member(&self, member: Option<&Member>) {
        if let Some(member) = member {
            lock(&self.names).insert(member.user.id.get(), name_of(member));
        }
    }
}

/// The one voice seat Discord gives a bot in each server, by server.
#[derive(Default)]
struct Seats(Mutex<HashMap<Id<GuildMarker>, Seat>>);

impl Seats {
    fn taken(&self, guild: Id<GuildMarker>) -> Option<String> {
        lock(&self.0)
            .get(&guild)
            .filter(|seat| seat.held())
            .map(|seat| seat.place.clone())
    }

    /// Takes the server's seat for `inbox`'s link, or names who holds it.
    fn claim(
        &self,
        guild: Id<GuildMarker>,
        channel: Id<ChannelMarker>,
        inbox: &Inbox,
    ) -> Option<String> {
        let mut seats = lock(&self.0);
        if let Some(seat) = seats
            .get(&guild)
            .filter(|seat| seat.held() && !seat.belongs_to(inbox))
        {
            return Some(seat.place.clone());
        }
        seats.insert(guild, inbox.seat(channel.to_string()));
        None
    }

    /// Whether `inbox`'s link still has the server's seat. A link that lost it to a newer one
    /// must not leave, since leaving takes the bot out of the newer link's channel.
    fn seated(&self, guild: Id<GuildMarker>, inbox: &Inbox) -> bool {
        lock(&self.0)
            .get(&guild)
            .is_some_and(|seat| seat.belongs_to(inbox))
    }
}

fn talker(names: &HashMap<u64, String>, user: u64) -> Talker {
    let id = user.to_string();
    let name = names.get(&user).cloned().unwrap_or_else(|| id.clone());
    Talker {
        id,
        name,
        verified: true,
    }
}

/// A voice source Discord has not tied to a user yet, named by its SSRC.
fn unmapped(ssrc: u32) -> Talker {
    let name = format!("ssrc {ssrc}");
    Talker {
        id: name.clone(),
        name,
        verified: false,
    }
}

fn name_of(member: &Member) -> String {
    member
        .nick
        .clone()
        .or_else(|| member.user.global_name.clone())
        .unwrap_or_else(|| member.user.name.clone())
}

fn lookup_failed(error: &twilight_http::Error) -> String {
    match error.kind() {
        ErrorType::Unauthorized => "bad bot token".into(),
        ErrorType::Response { status, .. } if status.get() == 404 => "no such channel".into(),
        ErrorType::Response { status, .. } if status.get() == 403 => "no access to channel".into(),
        _ => format!("Discord: {error}"),
    }
}

/// twilight builds its TLS configs from the process default provider, which rustls cannot pick
/// by itself once more than one provider is compiled in.
pub(crate) fn install_tls_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    }
}

fn songbird_config() -> Config {
    Config::default().decode_mode(DecodeMode::Decode(DecodeConfig::new(
        Channels::Mono,
        SampleRate::Hz48000,
    )))
}

async fn run_gateway(
    token: String,
    gateway: Arc<Gateway>,
    voice: watch::Sender<Voice>,
    alive: mpsc::Sender<()>,
) {
    let mut backoff = Backoff::new();
    loop {
        let Some(reason) = run_shard(&token, &gateway, &voice, &mut backoff, &alive).await else {
            return;
        };
        voice.send_replace(Voice::Failed(reason));
        tokio::select! {
            () = tokio::time::sleep(backoff.step()) => {}
            () = alive.closed() => return,
        }
    }
}

/// Runs one shard until it closes for good, with why, or until the session ends.
async fn run_shard(
    token: &str,
    gateway: &Gateway,
    voice: &watch::Sender<Voice>,
    backoff: &mut Backoff,
    alive: &mpsc::Sender<()>,
) -> Option<String> {
    let mut shard = Shard::new(
        ShardId::ONE,
        token.to_owned(),
        Intents::GUILDS | Intents::GUILD_VOICE_STATES,
    );
    let senders = Arc::new(TwilightMap::new(HashMap::from([(
        ShardId::ONE.number(),
        shard.sender(),
    )])));
    let mut songbird: Option<Arc<Songbird>> = None;
    let mut closed = None;
    loop {
        let item = tokio::select! {
            item = shard.next_event(WANTED) => item,
            () = alive.closed() => {
                goodbye(&mut shard, songbird.as_deref()).await;
                return None;
            }
        };
        let Some(item) = item else {
            return Some(closed.unwrap_or_else(|| "Discord closed".to_owned()));
        };
        let event = match item {
            Ok(event) => event,
            Err(error) => {
                tracing::warn!(%error, "Discord gateway");
                continue;
            }
        };
        match &event {
            GatewayEvent::Ready(ready) if songbird.is_none() => {
                let fresh = Arc::new(Songbird::twilight_from_config(
                    Arc::clone(&senders),
                    ready.user.id,
                    songbird_config(),
                ));
                voice.send_replace(Voice::Ready(Arc::clone(&fresh)));
                songbird = Some(fresh);
                backoff.reset();
            }
            GatewayEvent::GatewayClose(Some(frame)) => {
                closed = Some(format!("Discord closed: {} {}", frame.code, frame.reason));
            }
            GatewayEvent::GuildCreate(guild) => gateway.learn_guild(guild),
            GatewayEvent::VoiceStateUpdate(update) => {
                gateway.learn_member(update.0.member.as_ref());
            }
            _ => {}
        }
        if let Some(songbird) = &songbird {
            songbird.process(&event).await;
        }
    }
}

/// Leaves every voice channel and closes the shard, waiting briefly so Discord hears it.
async fn goodbye(shard: &mut Shard, songbird: Option<&Songbird>) {
    if let Some(songbird) = songbird {
        let guilds: Vec<_> = songbird.iter().map(|(guild, _)| guild).collect();
        for guild in guilds {
            if let Err(error) = songbird.remove(guild).await {
                tracing::debug!(%error, "Discord leave");
            }
        }
    }
    shard.close(CloseFrame::NORMAL);
    let flushed = tokio::time::timeout(GOODBYE, async {
        while let Some(item) = shard.next_event(WANTED).await {
            if matches!(item, Ok(GatewayEvent::GatewayClose(_))) {
                break;
            }
        }
    })
    .await;
    if flushed.is_err() {
        tracing::debug!("Discord close timed out");
    }
}

/// A paused-or-playing source for songbird: frames from `send` as raw f32, silence while
/// none are queued, the end once the sender is gone. Each silent gap would otherwise add its
/// length to the delay for the rest of the spurt, so a backlog past [`BACKLOG`] drops.
struct Feed {
    frames: Mutex<std_mpsc::Receiver<Vec<f32>>>,
    queue: VecDeque<Vec<f32>>,
    bytes: Vec<u8>,
    at: usize,
}

fn feed() -> (std_mpsc::Sender<Vec<f32>>, Feed) {
    let (frames_tx, frames) = std_mpsc::channel();
    let feed = Feed {
        frames: Mutex::new(frames),
        queue: VecDeque::with_capacity(BACKLOG * 2),
        bytes: Vec::with_capacity(FRAME * 4),
        at: 0,
    };
    (frames_tx, feed)
}

impl Feed {
    fn refill(&mut self) -> bool {
        if !self.collect() && self.queue.is_empty() {
            return false;
        }
        let dropped = self.queue.len().saturating_sub(BACKLOG);
        if dropped > 0 {
            self.queue.drain(..dropped);
            tracing::warn!(dropped, "Discord voice fell behind, frames dropped");
        }
        self.bytes.clear();
        self.at = 0;
        if let Some(frame) = self.queue.pop_front() {
            self.bytes
                .extend(frame.iter().flat_map(|sample| sample.to_le_bytes()));
        }
        if self.bytes.is_empty() {
            self.bytes.resize(FRAME * 4, 0);
        }
        true
    }

    /// Moves every waiting frame into the queue; false once `send` is gone.
    fn collect(&mut self) -> bool {
        let frames = self
            .frames
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        loop {
            match frames.try_recv() {
                Ok(frame) => self.queue.push_back(frame),
                Err(std_mpsc::TryRecvError::Empty) => return true,
                Err(std_mpsc::TryRecvError::Disconnected) => return false,
            }
        }
    }
}

impl Read for Feed {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.at == self.bytes.len() && !self.refill() {
            return Ok(0);
        }
        let count = buf.len().min(self.bytes.len() - self.at);
        buf[..count].copy_from_slice(&self.bytes[self.at..self.at + count]);
        self.at += count;
        Ok(count)
    }
}

impl Seek for Feed {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

impl MediaSource for Feed {
    fn is_seekable(&self) -> bool {
        false
    }

    fn byte_len(&self) -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_plays_frames_then_silence_until_send_is_gone() {
        let (frames, mut feed) = feed();
        frames.send(vec![0.5, -0.25]).unwrap();
        let mut buf = vec![0xff; FRAME * 4];
        assert_eq!(feed.read(&mut buf).unwrap(), 8);
        assert_eq!(buf[..4], 0.5_f32.to_le_bytes());
        assert_eq!(buf[4..8], (-0.25_f32).to_le_bytes());
        assert_eq!(feed.read(&mut buf).unwrap(), FRAME * 4);
        assert!(buf.iter().all(|&byte| byte == 0));
        drop(frames);
        assert_eq!(feed.read(&mut buf).unwrap(), 0);
    }

    #[test]
    fn feed_drops_a_backlog_past_a_hundred_ms() {
        let (frames, mut feed) = feed();
        for frame in 0..BACKLOG + 3 {
            frames.send(vec![frame as f32; 1]).unwrap();
        }
        let mut buf = [0; 4];
        assert_eq!(feed.read(&mut buf).unwrap(), 4);
        assert_eq!(f32::from_le_bytes(buf), 3.0);
    }

    #[test]
    fn unmapped_voice_is_named_by_ssrc_and_unverified() {
        let talker = unmapped(42);
        assert_eq!(talker.id, "ssrc 42");
        assert!(!talker.verified);
    }

    const GUILD: Id<GuildMarker> = Id::new(1);
    const LOBBY: Id<ChannelMarker> = Id::new(10);
    const OPS: Id<ChannelMarker> = Id::new(11);

    #[test]
    fn a_second_channel_in_one_server_clashes() {
        let seats = Seats::default();
        let (_first, first) = link::link();
        let (_second, second) = link::link();
        assert_eq!(seats.claim(GUILD, LOBBY, &first.inbox), None);
        assert_eq!(seats.taken(GUILD).as_deref(), Some("10"));
        assert_eq!(
            seats.claim(GUILD, OPS, &second.inbox).as_deref(),
            Some("10")
        );
        assert_eq!(seats.claim(GUILD, LOBBY, &first.inbox), None);
        assert!(seats.seated(GUILD, &first.inbox));
        assert!(!seats.seated(GUILD, &second.inbox));
    }

    #[test]
    fn a_released_link_hands_its_seat_on_at_once() {
        let seats = Seats::default();
        let (first, old) = link::link();
        let (_second, new) = link::link();
        assert_eq!(seats.claim(GUILD, LOBBY, &old.inbox), None);
        let Link { heard, hold, .. } = first;
        drop(hold);
        assert_eq!(seats.taken(GUILD), None);
        assert_eq!(seats.claim(GUILD, OPS, &new.inbox), None);
        assert_eq!(seats.taken(GUILD).as_deref(), Some("11"));
        assert!(!seats.seated(GUILD, &old.inbox));
        assert!(seats.seated(GUILD, &new.inbox));
        drop(heard);
    }

    #[tokio::test(start_paused = true)]
    async fn a_channel_change_hands_the_seat_over_without_leaving() {
        let seats = Seats::default();
        let (first, old) = link::link();
        let (_second, new) = link::link();
        assert_eq!(seats.claim(GUILD, LOBBY, &old.inbox), None);
        drop(first);
        let successor = async {
            tokio::time::sleep(HANDOVER / 2).await;
            seats.claim(GUILD, OPS, &new.inbox)
        };
        let (left, claimed) = tokio::join!(
            leaves(&seats, GUILD, &old.inbox, &Ended::Released),
            successor
        );
        assert_eq!(claimed, None);
        assert!(!left);
    }

    #[tokio::test(start_paused = true)]
    async fn a_released_link_with_no_successor_leaves() {
        let seats = Seats::default();
        let (first, old) = link::link();
        assert_eq!(seats.claim(GUILD, LOBBY, &old.inbox), None);
        drop(first);
        assert!(leaves(&seats, GUILD, &old.inbox, &Ended::Released).await);
    }

    #[test]
    fn unknown_discord_user_is_named_by_id() {
        let names = HashMap::from([(7, "Ann".to_owned())]);
        assert_eq!(talker(&names, 7).name, "Ann");
        assert_eq!(
            talker(&names, 9),
            Talker {
                id: "9".into(),
                name: "9".into(),
                verified: true,
            }
        );
    }
}

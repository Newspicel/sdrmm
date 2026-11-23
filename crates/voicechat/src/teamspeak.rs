//! `TeamSpeak` voice: one client per session, moved between channels, Opus both ways.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::timeout;
use tsclient_rs::{ChannelInfo, Client, ClientInfo, ClientOptions, Event, VoiceData};

use crate::link::{self, Backoff, Ends, Inbox, Outgoing, Seat};
use crate::opus::{Decoder, Encoder, Framer};
use crate::{Heard, Link, LinkState, Talker, VoiceChatError, lock};

const OPUS_VOICE: i32 = 4;
const OPUS_MUSIC: i32 = 5;
/// The codec we send. `TeamSpeak` clients level Opus Voice on playback, which undoes the
/// node's volume and pumps radio audio; they leave Opus Music as sent.
const SENT_CODEC: i32 = OPUS_MUSIC;
/// The handshake includes the server's proof of work puzzle, so it gets a while.
const HANDSHAKE: Duration = Duration::from_secs(20);
/// How long a released connection stays up for the session's next link, so a channel change
/// moves the client instead of leaving the server and connecting again.
const HANDOVER: Duration = Duration::from_secs(1);
/// What a clash calls a link that sits in the server's default channel.
const DEFAULT_CHANNEL: &str = "default channel";

pub(crate) struct Account {
    pub(crate) address: String,
    pub(crate) password: Option<String>,
    pub(crate) nickname: String,
    pub(crate) identity: String,
}

/// A `TeamSpeak` client sits in one channel, so the session holds one seat.
pub(crate) struct TeamSpeak {
    account: Arc<Account>,
    seat: Mutex<Option<Seat>>,
    /// Hands the next link to the connection already running, if it still waits for one.
    handover: Mutex<Option<mpsc::Sender<Place>>>,
    alive: mpsc::Sender<()>,
    _alive: mpsc::Receiver<()>,
}

impl TeamSpeak {
    pub(crate) fn new(account: Account) -> Self {
        let (alive, keep) = mpsc::channel(1);
        Self {
            account: Arc::new(account),
            seat: Mutex::new(None),
            handover: Mutex::new(None),
            alive,
            _alive: keep,
        }
    }

    pub(crate) fn join(&self, channel: &str) -> Result<Link, VoiceChatError> {
        let channel = channel.trim();
        let mut seat = lock(&self.seat);
        if let Some(taken) = seat.as_ref().filter(|seat| seat.held()) {
            return Err(VoiceChatError::Clash(taken.place.clone()));
        }
        let (link, ends) = link::link();
        let shown = if channel.is_empty() {
            DEFAULT_CHANNEL
        } else {
            channel
        };
        let taken = ends.inbox.seat(shown.to_owned());
        let place = Place {
            channel: channel.to_owned(),
            ends,
        };
        if let Err(place) = self.hand_over(place) {
            self.connect(place)?;
        }
        *seat = Some(taken);
        Ok(link)
    }

    fn hand_over(&self, place: Place) -> Result<(), Place> {
        match lock(&self.handover).as_ref() {
            Some(next) => next
                .try_send(place)
                .map_err(mpsc::error::TrySendError::into_inner),
            None => Err(place),
        }
    }

    fn connect(&self, place: Place) -> Result<(), VoiceChatError> {
        let (next, takeovers) = mpsc::channel(1);
        let account = Arc::clone(&self.account);
        let alive = self.alive.clone();
        std::thread::Builder::new()
            .name("teamspeak".into())
            .spawn(move || run_on_own_thread(&account, place, takeovers, &alive))
            .map_err(|error| VoiceChatError::Start(error.to_string()))?;
        *lock(&self.handover) = Some(next);
        Ok(())
    }
}

/// The channel a link wants, empty for the server's default, and the connection's ends of it.
struct Place {
    channel: String,
    ends: Ends,
}

/// The session's next link, if one comes within [`HANDOVER`]. Closing first keeps a link
/// handed over at the last moment from getting lost.
async fn handed_over(takeovers: &mut mpsc::Receiver<Place>) -> Option<Place> {
    if let Ok(Some(next)) = timeout(HANDOVER, takeovers.recv()).await {
        return Some(next);
    }
    takeovers.close();
    takeovers.try_recv().ok()
}

/// tsclient-rs holds std locks across awaits, so its futures are not `Send` and each
/// connection drives its own single-thread runtime.
fn run_on_own_thread(
    account: &Account,
    place: Place,
    takeovers: mpsc::Receiver<Place>,
    alive: &mpsc::Sender<()>,
) {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(run(account, place, takeovers, alive)),
        Err(error) => place
            .ends
            .inbox
            .set(LinkState::Failed(format!("runtime: {error}"))),
    }
}

enum Ended {
    Released,
    /// The next link wants the server's default channel, which only a fresh connection finds.
    Redial,
    Failed(String),
}

async fn run(
    account: &Account,
    mut place: Place,
    mut takeovers: mpsc::Receiver<Place>,
    alive: &mpsc::Sender<()>,
) {
    let mut backoff = Backoff::new();
    loop {
        let released =
            match connected(account, &mut place, &mut takeovers, &mut backoff, alive).await {
                Ended::Released => true,
                Ended::Redial => false,
                Ended::Failed(reason) => {
                    place.ends.inbox.set(LinkState::Failed(reason));
                    tokio::select! {
                        () = tokio::time::sleep(backoff.step()) => false,
                        () = place.ends.inbox.released() => true,
                        () = alive.closed() => return,
                        never = place.ends.outbox.discard() => match never {},
                    }
                }
            };
        if released {
            match handed_over(&mut takeovers).await {
                Some(next) => place = next,
                None => return,
            }
        }
    }
}

/// One connection, from handshake to leaving, moved to each link handed over meanwhile.
async fn connected(
    account: &Account,
    place: &mut Place,
    takeovers: &mut mpsc::Receiver<Place>,
    backoff: &mut Backoff,
    alive: &mpsc::Sender<()>,
) -> Ended {
    let Place { channel, ends } = place;
    let Ok(identity) = tsclient_rs::identityFromString(&account.identity) else {
        return Ended::Failed("bad identity".into());
    };
    let mut client = Client::new(
        identity,
        account.address.clone(),
        account.nickname.clone(),
        options(account, channel),
    );
    let mut events = forward_events(&client);
    let shaken = tokio::select! {
        shaken = handshake(&mut client, channel) => Some(shaken),
        () = ends.inbox.released() => None,
        () = alive.closed() => None,
        never = ends.outbox.discard() => match never {},
    };
    let live = matches!(shaken, Some(Ok(())));
    let lobby = (live && channel.is_empty()).then(|| client.channel_id());
    let mut ended = match shaken {
        Some(Ok(())) => {
            backoff.reset();
            joined(&client, &mut events, ends, alive).await
        }
        Some(Err(reason)) => Ended::Failed(reason),
        None => Ended::Released,
    };
    while live
        && matches!(ended, Ended::Released)
        && let Some(next) = handed_over(takeovers).await
    {
        *place = next;
        ended = if place.channel.is_empty() && lobby.is_none() {
            Ended::Redial
        } else {
            match move_to(&client, &place.channel, lobby).await {
                Ok(()) => joined(&client, &mut events, &mut place.ends, alive).await,
                Err(reason) => Ended::Failed(reason),
            }
        };
    }
    if let Err(error) = client.disconnect().await {
        tracing::debug!(%error, "TeamSpeak disconnect");
    }
    ended
}

async fn joined(
    client: &Client,
    events: &mut mpsc::UnboundedReceiver<Event>,
    ends: &mut Ends,
    alive: &mpsc::Sender<()>,
) -> Ended {
    ends.outbox.clear();
    ends.inbox.set(LinkState::Joined);
    talk(client, events, ends, alive).await
}

/// Without a channel the client asks for none, and the server seats it in its default.
fn options(account: &Account, channel: &str) -> ClientOptions {
    ClientOptions {
        server_password: account.password.clone(),
        default_channel: (!channel.is_empty()).then(|| channel.to_owned()),
        ..ClientOptions::default()
    }
}

/// Moves to `channel`, or to `lobby`, the default channel this connection landed in, when
/// `channel` is empty.
async fn move_to(client: &Client, channel: &str, lobby: Option<u64>) -> Result<(), String> {
    let id = match lobby.filter(|_| channel.is_empty()) {
        Some(lobby) => lobby,
        None => named(client, channel)?,
    };
    if client.channel_id() == id {
        return Ok(());
    }
    tsclient_rs::clientMove(client, client.client_id(), id, "")
        .await
        .map_err(|error| format!("move: {error}"))
}

fn named(client: &Client, channel: &str) -> Result<u64, String> {
    let directory = client
        .directory_snapshot()
        .ok_or_else(|| "no channel list".to_owned())?;
    channel_at(&directory.channels, channel).ok_or_else(|| format!("no channel {channel}"))
}

/// The channel a name or a `/` path names, walking down from the top. A bare name the top
/// level lacks matches a channel of that name anywhere.
fn channel_at(channels: &[ChannelInfo], path: &str) -> Option<u64> {
    let names: Vec<&str> = path.split('/').map(str::trim).collect();
    let walked = names.iter().try_fold(0, |parent, name| {
        channels
            .iter()
            .find(|info| info.parent_id == parent && info.name == *name)
            .map(|info| info.id)
    });
    match names.as_slice() {
        [name] => walked.or_else(|| {
            channels
                .iter()
                .find(|info| info.name == *name)
                .map(|info| info.id)
        }),
        _ => walked,
    }
}

/// Bridges the client's sync handlers onto one queue for the connection task.
fn forward_events(client: &Client) -> mpsc::UnboundedReceiver<Event> {
    let (tx, events) = mpsc::unbounded_channel();
    let forward: Arc<dyn Fn(Event) + Send + Sync> = Arc::new(move |event| {
        let _ = tx.send(event);
    });
    client
        .on_voice_data(Arc::clone(&forward))
        .on_client_enter(Arc::clone(&forward))
        .on_client_updated(Arc::clone(&forward))
        .on_client_leave(Arc::clone(&forward))
        .on_kicked(Arc::clone(&forward))
        .on_disconnected(forward);
    events
}

async fn handshake(client: &mut Client, channel: &str) -> Result<(), String> {
    let timed_out = |_| "connect timed out".to_owned();
    timeout(HANDSHAKE, client.connect())
        .await
        .map_err(timed_out)?
        .map_err(|error| error.to_string())?;
    timeout(HANDSHAKE, client.wait_connected(None))
        .await
        .map_err(timed_out)?
        .map_err(|error| error.to_string())?;
    timeout(HANDSHAKE, client.wait_directory_ready(None))
        .await
        .map_err(timed_out)?
        .map_err(|error| error.to_string())?;
    if landed_in(client, channel) {
        Ok(())
    } else {
        Err(format!("no channel {channel}"))
    }
}

/// The server drops a client whose default channel is missing into its default channel instead.
fn landed_in(client: &Client, channel: &str) -> bool {
    if channel.is_empty() {
        return true;
    }
    let wanted = channel.rsplit('/').next().unwrap_or(channel).trim();
    let here = client.channel_id();
    client.directory_snapshot().is_none_or(|directory| {
        directory
            .channels
            .iter()
            .any(|info| info.id == here && info.name == wanted)
    })
}

async fn talk(
    client: &Client,
    events: &mut mpsc::UnboundedReceiver<Event>,
    ends: &mut Ends,
    alive: &mpsc::Sender<()>,
) -> Ended {
    let mut voice = match Voice::new() {
        Ok(voice) => voice,
        Err(error) => return Ended::Failed(format!("opus: {error}")),
    };
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Some(event) => {
                    if let Some(ended) = voice.on_event(client, event, &ends.inbox) {
                        return ended;
                    }
                }
                None => return Ended::Failed("connection lost".into()),
            },
            outgoing = ends.outbox.next() => voice.say(client, outgoing),
            () = ends.inbox.released() => return Ended::Released,
            () = alive.closed() => return Ended::Released,
        }
    }
}

/// Both directions of one connection's audio.
struct Voice {
    encoder: Encoder,
    framer: Framer,
    decoders: HashMap<i32, Decoder>,
    talkers: HashMap<i32, Talker>,
}

impl Voice {
    fn new() -> Result<Self, opus::Error> {
        Ok(Self {
            encoder: Encoder::new()?,
            framer: Framer::default(),
            decoders: HashMap::new(),
            talkers: HashMap::new(),
        })
    }

    fn on_event(&mut self, client: &Client, event: Event, inbox: &Inbox) -> Option<Ended> {
        match event {
            Event::VoiceData(data) => self.hear(client, &data, inbox),
            Event::ClientEnter(info) | Event::ClientUpdated(info) => self.meet(&info),
            Event::ClientLeave(left) => {
                self.talkers.remove(&left.id);
                self.decoders.remove(&left.id);
            }
            Event::Kicked(reason) => return Some(Ended::Failed(format!("kicked: {reason}"))),
            Event::Disconnected(error) => {
                let reason = error.map_or_else(|| "disconnected".to_owned(), |e| e.to_string());
                return Some(Ended::Failed(reason));
            }
            _ => {}
        }
        None
    }

    fn meet(&mut self, info: &ClientInfo) {
        self.talkers.insert(
            info.id,
            Talker {
                id: info.uid.clone(),
                name: info.nickname.clone(),
                verified: true,
            },
        );
    }

    fn hear(&mut self, client: &Client, data: &VoiceData, inbox: &Inbox) {
        if data.data.is_empty() {
            return;
        }
        if data.codec != OPUS_VOICE && data.codec != OPUS_MUSIC {
            inbox.lose("codec other than Opus");
            return;
        }
        let talker = self.talker(client, data.client_id);
        let decoder = match self.decoders.entry(data.client_id) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => match Decoder::new() {
                Ok(decoder) => entry.insert(decoder),
                Err(error) => {
                    tracing::warn!(%error, "TeamSpeak opus decoder");
                    inbox.lose("no opus decoder");
                    return;
                }
            },
        };
        match decoder.decode(&data.data) {
            Ok(pcm) => inbox.hear(Heard { talker, pcm }),
            Err(_) => inbox.lose("undecodable opus frame"),
        }
    }

    /// The talker behind a client id, looked up in the directory the first time. A client
    /// the directory does not know yet is named by its id, unverified.
    fn talker(&mut self, client: &Client, id: i32) -> Talker {
        if !self.talkers.contains_key(&id)
            && let Some(directory) = client.directory_snapshot()
            && let Some(info) = directory.clients.iter().find(|info| info.id == id)
        {
            self.meet(info);
        }
        self.talkers.get(&id).cloned().unwrap_or_else(|| {
            let name = format!("client {id}");
            Talker {
                id: name.clone(),
                name,
                verified: false,
            }
        })
    }

    fn say(&mut self, client: &Client, outgoing: Outgoing) {
        match outgoing {
            Outgoing::Frame(samples) => {
                self.framer.push(&samples);
                while let Some(frame) = self.framer.pop() {
                    self.send(client, &frame);
                }
            }
            Outgoing::Idle => {
                if let Some(frame) = self.framer.flush() {
                    self.send(client, &frame);
                }
                client.send_voice(Vec::new(), SENT_CODEC);
            }
        }
    }

    fn send(&mut self, client: &Client, frame: &[f32]) {
        match self.encoder.encode(frame) {
            Ok(packet) => client.send_voice(packet, SENT_CODEC),
            Err(error) => tracing::warn!(%error, "TeamSpeak opus encoder"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(id: u64, parent_id: u64, name: &str) -> ChannelInfo {
        ChannelInfo {
            name: name.into(),
            description: String::new(),
            id,
            parent_id,
            order: 0,
            password_protected: None,
            max_clients: None,
            max_clients_limited: None,
            codec: None,
            icon_id: None,
        }
    }

    fn session() -> TeamSpeak {
        TeamSpeak::new(Account {
            address: "127.0.0.1:9".into(),
            password: None,
            nickname: "sdrmm".into(),
            identity: crate::new_teamspeak_identity(),
        })
    }

    #[test]
    fn a_path_walks_down_from_the_top() {
        let channels = [
            channel(1, 0, "Ops"),
            channel(2, 1, "Radio"),
            channel(3, 0, "Radio"),
            channel(4, 1, "Deep"),
        ];
        assert_eq!(channel_at(&channels, "Ops/Radio"), Some(2));
        assert_eq!(channel_at(&channels, "Radio"), Some(3));
        assert_eq!(channel_at(&channels, "Deep"), Some(4));
        assert_eq!(channel_at(&channels, "Ops/Lobby"), None);
    }

    #[test]
    fn an_empty_channel_asks_for_the_server_default() {
        let account = Account {
            address: "127.0.0.1:9".into(),
            password: None,
            nickname: "sdrmm".into(),
            identity: String::new(),
        };
        assert_eq!(options(&account, "").default_channel, None);
        assert_eq!(
            options(&account, "Ops").default_channel.as_deref(),
            Some("Ops")
        );
    }

    #[test]
    fn an_empty_channel_clashes_as_the_default_channel() {
        let session = session();
        let _lobby = session.join("  ").unwrap();
        assert_eq!(
            session.join("Ops").unwrap_err(),
            VoiceChatError::Clash(DEFAULT_CHANNEL.into())
        );
    }

    #[test]
    fn a_new_link_goes_to_the_waiting_connection() {
        let session = session();
        let (next, mut takeovers) = mpsc::channel(1);
        *lock(&session.handover) = Some(next);
        let _link = session.join("Ops").unwrap();
        assert_eq!(takeovers.try_recv().unwrap().channel, "Ops");
    }

    #[tokio::test(start_paused = true)]
    async fn a_released_connection_waits_for_the_next_link() {
        let (next, mut takeovers) = mpsc::channel(1);
        let (_link, ends) = link::link();
        let late = async {
            tokio::time::sleep(HANDOVER / 2).await;
            next.try_send(Place {
                channel: "Ops".into(),
                ends,
            })
        };
        let (handed, sent) = tokio::join!(handed_over(&mut takeovers), late);
        assert!(sent.is_ok());
        assert_eq!(handed.map(|place| place.channel).as_deref(), Some("Ops"));
    }

    #[tokio::test(start_paused = true)]
    async fn no_next_link_ends_the_connection_and_refuses_more() {
        let (next, mut takeovers) = mpsc::channel(1);
        assert!(handed_over(&mut takeovers).await.is_none());
        let (_link, ends) = link::link();
        let refused = next.try_send(Place {
            channel: "Ops".into(),
            ends,
        });
        assert!(refused.is_err());
    }
}

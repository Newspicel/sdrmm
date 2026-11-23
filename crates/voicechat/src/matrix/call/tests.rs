use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use super::homeserver::Homeserver;
use super::media::Media;
use super::people::Me;
use super::wire::{Dial, Role, Wire, WireEvent};
use super::*;
use crate::link::{self, Link};
use crate::matrix::e2ee::keys::Outgoing;
use crate::matrix::focus::LiveKitGrant;
use crate::matrix::livekit::peer::OpusOut;
use crate::matrix::membership::{Focus, StateEntry};
use crate::opus::Encoder;
use crate::{FRAME, LinkState, Talker};

const ROOM: &str = "!room:x.org";
const FOCUS: &str = "https://lk.x.org";
const ALICE: &str = "@alice:x.org";
const ALICE_ID: &str = "@alice:x.org:ALICE";
const PATIENCE: Duration = Duration::from_secs(5);

fn me() -> Me {
    Me {
        user: "@bot:x.org".into(),
        device: "BOT".into(),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn member_state(user: &str, device: &str) -> CallState {
    CallState {
        entry: (
            "org.matrix.msc3401.call.member".into(),
            format!("_{user}_{device}_m.call"),
            json!({
                "application": "m.call",
                "call_id": "",
                "scope": "m.room",
                "device_id": device,
                "created_ts": now_ms() - 1000,
                "expires": 14_400_000,
                "focus_active": {"type": "livekit", "focus_selection": "multi_sfu"},
                "foci_preferred": [{"type": "livekit", "livekit_service_url": FOCUS}],
            }),
        ),
        sent_ms: now_ms() - 1000,
    }
}

fn left_state(user: &str, device: &str) -> CallState {
    let mut state = member_state(user, device);
    state.entry.2 = json!({});
    state
}

#[derive(Debug, Clone, PartialEq)]
enum Sent {
    State(StateEntry),
    Delayed(StateEntry),
    Restart(String),
    Cancel(String),
    Keys(Vec<(String, String)>, EncryptionKeys),
}

#[derive(Default)]
struct Server {
    encrypted: bool,
    delayed: bool,
    states: Vec<CallState>,
    sent: Mutex<Vec<Sent>>,
}

impl Server {
    fn log(&self, sent: Sent) {
        self.sent.lock().unwrap().push(sent);
    }

    fn sent(&self) -> Vec<Sent> {
        self.sent.lock().unwrap().clone()
    }

    fn keys_sent(&self) -> Vec<(Vec<(String, String)>, EncryptionKeys)> {
        self.sent()
            .into_iter()
            .filter_map(|sent| match sent {
                Sent::Keys(to, content) => Some((to, content)),
                _ => None,
            })
            .collect()
    }
}

#[async_trait]
impl Homeserver for Server {
    fn room_id(&self) -> String {
        ROOM.into()
    }

    async fn foci(&self) -> Result<Vec<Focus>, String> {
        Ok(vec![Focus {
            service_url: FOCUS.into(),
        }])
    }

    async fn encrypted(&self) -> Result<bool, String> {
        Ok(self.encrypted)
    }

    fn owned_state_keys(&self) -> bool {
        false
    }

    async fn delayed_events(&self) -> bool {
        self.delayed
    }

    async fn call_states(&self) -> Result<Vec<CallState>, String> {
        Ok(self.states.clone())
    }

    async fn put_state(&self, entry: &StateEntry) -> Result<(), String> {
        self.log(Sent::State(entry.clone()));
        Ok(())
    }

    async fn put_delayed(&self, entry: &StateEntry, delay: Duration) -> Result<String, String> {
        assert_eq!(delay, Duration::from_secs(8));
        self.log(Sent::Delayed(entry.clone()));
        Ok("delay-1".into())
    }

    async fn restart_delayed(&self, id: &str) -> Result<(), String> {
        self.log(Sent::Restart(id.into()));
        Ok(())
    }

    async fn cancel_delayed(&self, id: &str) -> Result<(), String> {
        self.log(Sent::Cancel(id.into()));
        Ok(())
    }

    async fn send_keys(
        &self,
        to: &[(String, String)],
        content: &EncryptionKeys,
    ) -> Result<(), String> {
        self.log(Sent::Keys(to.to_vec(), content.clone()));
        Ok(())
    }

    async fn display_name(&self, user: &str) -> Option<String> {
        (user == ALICE).then(|| "Alice".to_owned())
    }
}

struct FakeWire {
    heard: mpsc::UnboundedReceiver<WireEvent>,
    said: mpsc::UnboundedSender<OpusOut>,
    left: Arc<AtomicBool>,
}

#[async_trait]
impl Wire for FakeWire {
    fn publish(&mut self, frame: OpusOut) {
        let _ = self.said.send(frame);
    }

    async fn next(&mut self) -> Result<WireEvent, String> {
        match self.heard.recv().await {
            Some(event) => Ok(event),
            None => std::future::pending().await,
        }
    }

    fn dropped(&self) -> u64 {
        0
    }

    async fn leave(&mut self) {
        self.left.store(true, Ordering::SeqCst);
    }
}

/// The SFU end of a fake wire.
struct Far {
    hear: mpsc::UnboundedSender<WireEvent>,
    said: mpsc::UnboundedReceiver<OpusOut>,
    left: Arc<AtomicBool>,
}

impl Far {
    fn say(&self, identity: &str, payload: Vec<u8>) {
        self.hear
            .send(WireEvent::Heard {
                identity: identity.into(),
                payload,
            })
            .unwrap();
    }
}

fn wire() -> (FakeWire, Far) {
    let (hear, heard) = mpsc::unbounded_channel();
    let (said_tx, said) = mpsc::unbounded_channel();
    let left = Arc::new(AtomicBool::new(false));
    let fake = FakeWire {
        heard,
        said: said_tx,
        left: Arc::clone(&left),
    };
    (fake, Far { hear, said, left })
}

struct Dialer {
    wires: Mutex<VecDeque<Result<FakeWire, String>>>,
    roles: Mutex<Vec<Role>>,
}

#[async_trait]
impl Dial for Dialer {
    async fn grant(&self, focus: &Focus) -> Result<LiveKitGrant, String> {
        Ok(LiveKitGrant {
            url: focus.service_url.clone(),
            jwt: "token".into(),
        })
    }

    async fn connect(&self, _: &LiveKitGrant, role: Role) -> Result<Box<dyn Wire>, String> {
        self.roles.lock().unwrap().push(role);
        let next = self.wires.lock().unwrap().pop_front();
        match next {
            Some(Ok(wire)) => Ok(Box::new(wire)),
            Some(Err(reason)) => Err(reason),
            None => Err("No call server".into()),
        }
    }
}

struct Call {
    link: Link,
    server: Arc<Server>,
    far: Far,
    sync: broadcast::Sender<SyncEvent>,
    task: JoinHandle<Ended>,
}

fn start_with(
    server: Server,
    dialed: Result<FakeWire, String>,
) -> (
    Link,
    Arc<Server>,
    broadcast::Sender<SyncEvent>,
    JoinHandle<Ended>,
) {
    let server = Arc::new(server);
    let (link, ends) = link::link();
    let (sync, events) = broadcast::channel(64);
    let dialer = Dialer {
        wires: Mutex::new(VecDeque::from([dialed])),
        roles: Mutex::new(Vec::new()),
    };
    let hs = Arc::clone(&server);
    let task = tokio::spawn(async move { run::run(&*hs, &dialer, me(), events, ends).await });
    (link, server, sync, task)
}

async fn start(server: Server) -> Call {
    let (fake, far) = wire();
    let (mut link, server, sync, task) = start_with(server, Ok(fake));
    joined(&mut link).await;
    Call {
        link,
        server,
        far,
        sync,
        task,
    }
}

async fn joined(link: &mut Link) {
    timeout(
        PATIENCE,
        link.state.wait_for(|state| *state == LinkState::Joined),
    )
    .await
    .expect("joined in time")
    .expect("state lives");
}

fn tone() -> Vec<f32> {
    (0..FRAME).map(|i| 0.5 * (i as f32 * 0.13).sin()).collect()
}

/// Alice's end: her media and the key event she would send us.
fn alice() -> (Media, Outgoing, Link) {
    let (link, ends) = link::link();
    let mut media = Media::new(true, ends.inbox.ear(), Instant::now()).unwrap();
    let key = media.keys().unwrap().on_joined();
    (media, key, link)
}

fn key_event(key: &Outgoing) -> SyncEvent {
    SyncEvent::Key {
        sender: ALICE.into(),
        device: Some("ALICE".into()),
        content: EncryptionKeys::new(ROOM, ALICE_ID, "ALICE", key),
    }
}

async fn wait_until(mut done: impl FnMut() -> bool) {
    timeout(PATIENCE, async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("in time");
}

#[tokio::test]
async fn a_clear_room_sends_frames_as_they_are() {
    let mut call = start(Server::default()).await;
    call.link.send.send(tone()).await.unwrap();
    let sent = timeout(PATIENCE, call.far.said.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        sent.payload,
        Encoder::new().unwrap().encode(&tone()).unwrap()
    );
}

#[tokio::test]
async fn an_encrypted_frame_opens_on_the_far_side() {
    let mut call = start(Server {
        encrypted: true,
        states: vec![member_state(ALICE, "ALICE")],
        ..Server::default()
    })
    .await;
    wait_until(|| !call.server.keys_sent().is_empty()).await;
    let (to, content) = call.server.keys_sent().remove(0);
    assert_eq!(to, [(ALICE.to_owned(), "ALICE".to_owned())]);
    assert_eq!(content.room_id, ROOM);
    assert_eq!(content.member.claimed_device_id, "BOT");

    call.link.send.send(tone()).await.unwrap();
    let sent = timeout(PATIENCE, call.far.said.recv())
        .await
        .unwrap()
        .unwrap();
    let clear = Encoder::new().unwrap().encode(&tone()).unwrap();
    assert_ne!(sent.payload, clear);

    let (mut alice, _, alice_link) = alice();
    let (index, material) = content.entry().unwrap();
    alice
        .keys()
        .unwrap()
        .receive("@bot:x.org:BOT", index, &material)
        .unwrap();
    let pcm = alice.incoming("@bot:x.org:BOT", &sent.payload).unwrap();
    assert_eq!(pcm.len(), FRAME);
    assert_eq!(alice_link.dropped(), 0);
}

#[tokio::test]
async fn audio_flows_once_the_key_arrives() {
    let mut call = start(Server {
        encrypted: true,
        states: vec![member_state(ALICE, "ALICE")],
        ..Server::default()
    })
    .await;
    let (mut alice, key, _alice_link) = alice();
    let mut frame = || alice.outgoing(&tone(), 960, Instant::now()).unwrap();

    call.far.say(ALICE_ID, frame());
    wait_until(|| call.link.dropped() == 1).await;
    assert!(call.link.heard.try_recv().is_err());

    call.sync.send(key_event(&key)).unwrap();
    let heard = timeout(PATIENCE, async {
        loop {
            call.far.say(ALICE_ID, frame());
            if let Ok(Some(heard)) =
                timeout(Duration::from_millis(50), call.link.heard.recv()).await
            {
                return heard;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        heard.talker,
        Talker {
            id: ALICE.into(),
            name: "Alice".into(),
            verified: true,
        }
    );
    assert_eq!(heard.pcm.len(), FRAME);
    assert!(call.link.dropped() >= 1);
}

#[tokio::test]
async fn an_unknown_identity_is_unverified() {
    let mut call = start(Server {
        states: vec![member_state(ALICE, "ALICE")],
        ..Server::default()
    })
    .await;
    let opus = Encoder::new().unwrap().encode(&tone()).unwrap();
    call.far.say("@mallory:x.org:EVIL", opus.clone());
    let heard = timeout(PATIENCE, call.link.heard.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(heard.talker.id, "@mallory:x.org:EVIL");
    assert!(!heard.talker.verified);

    call.far.say(ALICE_ID, opus.clone());
    let heard = timeout(PATIENCE, call.link.heard.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(heard.talker.id, ALICE);
    assert!(heard.talker.verified);

    call.far
        .hear
        .send(WireEvent::Left(ALICE_ID.into()))
        .unwrap();
    call.far.say(ALICE_ID, opus);
    let heard = timeout(PATIENCE, call.link.heard.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(heard.pcm.len(), FRAME);
}

#[tokio::test]
async fn a_key_from_another_device_is_unverified() {
    let mut call = start(Server {
        encrypted: true,
        states: vec![member_state(ALICE, "ALICE")],
        ..Server::default()
    })
    .await;
    let (mut alice, key, _alice_link) = alice();
    let SyncEvent::Key { content, .. } = key_event(&key) else {
        unreachable!()
    };
    call.sync
        .send(SyncEvent::Key {
            sender: ALICE.into(),
            device: Some("OTHER".into()),
            content,
        })
        .unwrap();
    let heard = timeout(PATIENCE, async {
        loop {
            call.far.say(
                ALICE_ID,
                alice.outgoing(&tone(), 960, Instant::now()).unwrap(),
            );
            if let Ok(Some(heard)) =
                timeout(Duration::from_millis(50), call.link.heard.recv()).await
            {
                return heard;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(heard.talker.id, ALICE);
    assert!(!heard.talker.verified);
}

#[tokio::test]
async fn a_member_leaving_rotates_our_key() {
    let bob = "@bob:y.org";
    let call = start(Server {
        encrypted: true,
        states: vec![member_state(ALICE, "ALICE"), member_state(bob, "BOB")],
        ..Server::default()
    })
    .await;
    wait_until(|| !call.server.keys_sent().is_empty()).await;
    let first = call.server.keys_sent()[0].1.entry().unwrap().0;
    call.sync
        .send(SyncEvent::Member {
            room: ROOM.into(),
            state: left_state(bob, "BOB"),
        })
        .unwrap();
    wait_until(|| call.server.keys_sent().len() == 2).await;
    let (to, content) = call.server.keys_sent().remove(1);
    assert_eq!(to, [(ALICE.to_owned(), "ALICE".to_owned())]);
    assert_eq!(content.entry().unwrap().0, first.wrapping_add(1));
}

#[tokio::test]
async fn leaving_sends_an_empty_membership() {
    let call = start(Server {
        delayed: true,
        ..Server::default()
    })
    .await;
    let Call {
        link,
        server,
        far,
        task,
        ..
    } = call;
    drop(link);
    let ended = timeout(PATIENCE, task).await.unwrap().unwrap();
    assert_eq!(ended, Ended::Released);
    assert!(far.left.load(Ordering::SeqCst));
    let sent = server.sent();
    let Sent::Delayed(leave) = &sent[0] else {
        panic!("{sent:?}")
    };
    assert_eq!(leave.2, json!({}));
    let Sent::State(join) = &sent[1] else {
        panic!("{sent:?}")
    };
    assert_eq!(join.1, leave.1);
    assert_eq!(join.2["device_id"], "BOT");
    assert!(sent.contains(&Sent::State(leave.clone())));
    assert_eq!(sent.last(), Some(&Sent::Cancel("delay-1".into())));
}

#[tokio::test]
async fn a_wire_that_cannot_connect_fails_the_link() {
    let (mut link, server, _sync, _task) =
        start_with(Server::default(), Err("No UDP route".into()));
    timeout(
        PATIENCE,
        link.state
            .wait_for(|state| *state == LinkState::Failed("No UDP route".into())),
    )
    .await
    .unwrap()
    .unwrap();
    wait_until(|| {
        server
            .sent()
            .iter()
            .any(|sent| matches!(sent, Sent::State(entry) if entry.2 == json!({})))
    })
    .await;
}

#[test]
fn both_key_event_types_are_heard() {
    let out = Outgoing {
        index: 2,
        material: [9; 16],
    };
    let content = serde_json::to_value(EncryptionKeys::new(ROOM, ALICE_ID, "ALICE", &out)).unwrap();
    for kind in ["io.element.call.encryption_keys", "m.rtc.encryption_key"] {
        let event = SyncEvent::key(kind, ALICE, Some("ALICE"), &content).unwrap();
        let SyncEvent::Key {
            sender,
            device,
            content,
        } = event
        else {
            panic!("not a key")
        };
        assert_eq!((sender.as_str(), device.as_deref()), (ALICE, Some("ALICE")));
        assert_eq!(content.entry(), Some((2, vec![9; 16])));
    }
    assert!(SyncEvent::key("m.room.message", ALICE, None, &content).is_none());
    assert!(SyncEvent::key("m.rtc.encryption_key", ALICE, None, &json!({})).is_none());
}

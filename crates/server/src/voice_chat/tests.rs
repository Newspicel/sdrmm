use std::time::Duration;

use sdrmm_engine::{PcmBlock, PcmPayload};
use sdrmm_voicechat::{Heard, Talker};
use sdrmm_wire::{
    ChatService, EncoderNode, PatchEdge, PatchNode, PortRef, Position, SendMode, VoiceChatState,
    VoiceGateSettings, event_output::Secret,
};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use super::*;

const ACCOUNT: &str = "club";
const CHANNEL: &str = "Lobby";

/// A chat end the test holds: what the node sends, and the way to speak to it.
struct FarEnd {
    joined: (String, String),
    heard: mpsc::Sender<Heard>,
    _sent: mpsc::Receiver<Vec<f32>>,
    _state: watch::Sender<LinkState>,
}

#[derive(Default)]
struct FakeLinks {
    ends: Mutex<Vec<FarEnd>>,
}

impl FakeLinks {
    fn joined(&self) -> Vec<(String, String)> {
        let ends = self.ends.lock().expect("ends");
        ends.iter().map(|end| end.joined.clone()).collect()
    }

    fn heard(&self) -> mpsc::Sender<Heard> {
        self.ends.lock().expect("ends")[0].heard.clone()
    }
}

impl Links for FakeLinks {
    fn join(&self, account: &ChatAccount, channel: &str) -> Result<Link, String> {
        let (send, sent) = mpsc::channel(64);
        let (heard, hears) = mpsc::channel(64);
        let (state, watched) = watch::channel(LinkState::Joined);
        self.ends.lock().expect("ends").push(FarEnd {
            joined: (account.id.clone(), channel.to_owned()),
            heard,
            _sent: sent,
            _state: state,
        });
        Ok(Link {
            send,
            heard: hears,
            state: watched,
            drops: Arc::default(),
            hold: sdrmm_voicechat::Hold::default(),
        })
    }

    fn settle(&self, _used: &[ChatAccount]) -> Vec<String> {
        Vec::new()
    }
}

fn account() -> ChatAccount {
    ChatAccount {
        id: ACCOUNT.to_owned(),
        name: "Club".to_owned(),
        service: ChatService::Discord {
            token: Secret::Inline("token".to_owned()),
        },
    }
}

fn node(id: &str, body: NodeBody) -> PatchNode {
    PatchNode {
        id: id.to_owned(),
        body,
        position: Position { x: 0.0, y: 0.0 },
        size: None,
        label: None,
    }
}

fn wire(from: &str, to: &str) -> PatchEdge {
    let port = |node: &str| PortRef {
        node: node.to_owned(),
        port: AUDIO_PORT.to_owned(),
    };
    PatchEdge {
        from: port(from),
        to: port(to),
    }
}

fn voice_chat(allowed: &[&str]) -> NodeBody {
    NodeBody::VoiceChat(VoiceChatNode {
        account: ACCOUNT.to_owned(),
        channel: CHANNEL.to_owned(),
        allowed: allowed.iter().map(|id| (*id).to_owned()).collect(),
        to_air: VoiceGateSettings {
            mode: SendMode::Continuous,
            ..VoiceGateSettings::default()
        },
        ..VoiceChatNode::default()
    })
}

/// Voice chat nodes, each wired into one encoder when `wired`.
fn graph(chats: Vec<(&str, NodeBody)>, wired: bool) -> PatchGraph {
    let mut graph = PatchGraph {
        nodes: vec![node("enc", NodeBody::Encoder(EncoderNode::default()))],
        edges: Vec::new(),
    };
    for (id, body) in chats {
        graph.nodes.push(node(id, body));
        if wired {
            graph.edges.push(wire(id, "enc"));
        }
    }
    graph
}

fn resolved(graph: PatchGraph, engine: &Engine) -> Resolved {
    Resolved {
        patch: crate::workspace::resolve(&graph, &engine.snapshot()),
        graph: Arc::new(graph),
        ..Resolved::default()
    }
}

fn chats(links: &Arc<FakeLinks>) -> VoiceChats {
    let mut chats = VoiceChats::new(links.clone());
    chats.set_accounts(vec![account()]);
    chats
}

fn on_air() -> (Air, UnboundedReceiver<Vec<f32>>) {
    let (tx, rx) = unbounded_channel();
    let air: Air = Arc::new(move |pcm: &[f32]| usize::from(tx.send(pcm.to_vec()).is_ok()));
    (air, rx)
}

fn settle(
    chats: &mut VoiceChats,
    engine: &Arc<Engine>,
    graph: PatchGraph,
    keying: bool,
    air: &Air,
) -> Vec<NodeProblem> {
    let current = resolved(graph, engine);
    let runtime = Handle::current();
    let ctx = Ctx {
        engine,
        runtime: &runtime,
        sync_fx: &|| {},
        keying: &|_| keying,
        air: &|_| air.clone(),
    };
    chats.settle(&ctx, &current)
}

fn said(id: &str, level: f32) -> Heard {
    Heard {
        talker: Talker {
            id: id.to_owned(),
            name: id.to_uppercase(),
            verified: true,
        },
        pcm: vec![level; FRAME],
    }
}

fn said_unverified(id: &str, level: f32) -> Heard {
    let mut heard = said(id, level);
    heard.talker.verified = false;
    heard
}

/// The first frame on the air that is not silence.
async fn first_voice(air: &mut UnboundedReceiver<Vec<f32>>) -> Vec<f32> {
    let voiced = async {
        loop {
            let frame = air.recv().await.expect("air");
            if frame.iter().any(|sample| *sample != 0.0) {
                return frame;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), voiced)
        .await
        .expect("no voice reached the air")
}

#[tokio::test]
async fn unwired_node_stays_in_its_channel() {
    let engine = Engine::new(None);
    let links = Arc::new(FakeLinks::default());
    let mut chats = chats(&links);
    let (air, _) = on_air();
    for wired in [false, true, false] {
        let problems = settle(
            &mut chats,
            &engine,
            graph(vec![("chat", voice_chat(&[]))], wired),
            false,
            &air,
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(links.joined(), [(ACCOUNT.to_owned(), CHANNEL.to_owned())]);
    }
}

fn in_channel(channel: &str) -> NodeBody {
    let NodeBody::VoiceChat(chat) = voice_chat(&[]) else {
        unreachable!("voice_chat builds a voice chat node");
    };
    NodeBody::VoiceChat(VoiceChatNode {
        channel: channel.to_owned(),
        ..chat
    })
}

fn teamspeak() -> ChatAccount {
    ChatAccount {
        service: ChatService::Teamspeak {
            address: "ts.example".to_owned(),
            password: Secret::default(),
            nickname: "sdrmm".to_owned(),
            identity: Secret::default(),
        },
        ..account()
    }
}

#[tokio::test]
async fn empty_channel_on_teamspeak_joins_the_default_channel() {
    let engine = Engine::new(None);
    let links = Arc::new(FakeLinks::default());
    let mut chats = VoiceChats::new(links.clone());
    chats.set_accounts(vec![teamspeak()]);
    let (air, _) = on_air();
    let problems = settle(
        &mut chats,
        &engine,
        graph(vec![("chat", in_channel("  "))], false),
        false,
        &air,
    );
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(links.joined(), [(ACCOUNT.to_owned(), String::new())]);
}

#[tokio::test]
async fn empty_channel_on_discord_stays_idle() {
    let engine = Engine::new(None);
    let links = Arc::new(FakeLinks::default());
    let mut chats = chats(&links);
    let (air, _) = on_air();
    let problems = settle(
        &mut chats,
        &engine,
        graph(vec![("chat", in_channel(""))], true),
        false,
        &air,
    );
    assert!(problems.is_empty(), "{problems:?}");
    assert!(links.joined().is_empty());
}

#[tokio::test]
async fn an_unknown_account_is_idle_without_a_fault() {
    let engine = Engine::new(None);
    let links = Arc::new(FakeLinks::default());
    let mut chats = VoiceChats::new(links.clone());
    let (air, _) = on_air();
    let wired = graph(vec![("chat", voice_chat(&[]))], true);
    let problems = settle(&mut chats, &engine, wired.clone(), false, &air);
    assert!(problems.is_empty(), "{problems:?}");
    assert!(links.joined().is_empty());
    let statuses = chats.statuses(&wired, &problems);
    assert_eq!(statuses, [status::idle("chat", None)]);
}

#[tokio::test]
async fn only_allowed_talkers_reach_the_transmitter() {
    let engine = Engine::new(None);
    let links = Arc::new(FakeLinks::default());
    let mut chats = chats(&links);
    let (air, mut aired) = on_air();
    settle(
        &mut chats,
        &engine,
        graph(vec![("chat", voice_chat(&["ann"]))], true),
        true,
        &air,
    );
    let heard = links.heard();
    heard.send(said("bob", 0.25)).await.expect("bob");
    heard.send(said("ann", 0.5)).await.expect("ann");
    assert_eq!(first_voice(&mut aired).await, vec![0.5; FRAME]);
}

#[tokio::test]
async fn a_stand_in_id_never_matches_the_allowed_list() {
    let engine = Engine::new(None);
    let links = Arc::new(FakeLinks::default());
    let mut chats = chats(&links);
    let (air, mut aired) = on_air();
    settle(
        &mut chats,
        &engine,
        graph(vec![("chat", voice_chat(&["ssrc 7", "ann"]))], true),
        true,
        &air,
    );
    let heard = links.heard();
    heard
        .send(said_unverified("ssrc 7", 0.25))
        .await
        .expect("ssrc");
    heard.send(said("ann", 0.5)).await.expect("ann");
    assert_eq!(first_voice(&mut aired).await, vec![0.5; FRAME]);
}

#[test]
fn a_stand_in_talker_shows_unverified_and_not_allowed() {
    let listed = VoiceChatNode {
        allowed: vec!["client 3".to_owned()],
        ..VoiceChatNode::default()
    };
    assert!(!to_air::allowed(&listed, "client 3", false));
    assert!(to_air::allowed(&listed, "client 3", true));
    assert!(to_air::allowed(
        &VoiceChatNode::default(),
        "client 3",
        false
    ));
    let mut to_air = to_air::ToAir::new(&VoiceGateSettings::default());
    let tuning = Tuning {
        node: listed.clone(),
        keying: false,
    };
    let (air, _) = on_air();
    let meters = Meters::default();
    let now = tokio::time::Instant::now();
    to_air.hear(
        said_unverified("client 3", 0.1),
        &tuning,
        now,
        &air,
        &meters,
    );
    let talking = to_air.talking(&listed);
    assert_eq!(talking.len(), 1);
    assert!(!talking[0].verified && !talking[0].allowed);
}

#[tokio::test]
async fn empty_allowed_lets_anyone_through() {
    let engine = Engine::new(None);
    let links = Arc::new(FakeLinks::default());
    let mut chats = chats(&links);
    let (air, mut aired) = on_air();
    settle(
        &mut chats,
        &engine,
        graph(vec![("chat", voice_chat(&[]))], true),
        true,
        &air,
    );
    links.heard().send(said("bob", 0.25)).await.expect("bob");
    assert_eq!(first_voice(&mut aired).await, vec![0.25; FRAME]);
}

#[tokio::test]
async fn nothing_reaches_the_air_unless_a_transmitter_is_armed() {
    let engine = Engine::new(None);
    let links = Arc::new(FakeLinks::default());
    let mut chats = chats(&links);
    let (air, mut aired) = on_air();
    settle(
        &mut chats,
        &engine,
        graph(vec![("chat", voice_chat(&[]))], true),
        false,
        &air,
    );
    links.heard().send(said("bob", 0.25)).await.expect("bob");
    let quiet = tokio::time::timeout(Duration::from_millis(200), aired.recv()).await;
    assert!(quiet.is_err(), "{quiet:?}");
}

#[tokio::test]
async fn same_account_same_channel_twice_is_a_fault() {
    let engine = Engine::new(None);
    let links = Arc::new(FakeLinks::default());
    let mut chats = chats(&links);
    let (air, _) = on_air();
    let both = graph(vec![("a", voice_chat(&[])), ("b", voice_chat(&[]))], true);
    let problems = settle(&mut chats, &engine, both.clone(), false, &air);
    assert_eq!(links.joined().len(), 1);
    assert_eq!(problems, [problem("b", CLASH.to_owned())]);
    let statuses = chats.statuses(&both, &problems);
    let second = statuses
        .iter()
        .find(|status| status.node == "b")
        .expect("b");
    assert_eq!(second.state, VoiceChatState::Idle);
    assert_eq!(second.error.as_deref(), Some(CLASH));
}

fn tuning(mode: SendMode) -> Tuning {
    Tuning {
        node: VoiceChatNode {
            to_chat: VoiceGateSettings {
                mode,
                ..VoiceGateSettings::default()
            },
            ..VoiceChatNode::default()
        },
        keying: false,
    }
}

#[tokio::test]
async fn continuous_to_chat_sends_silence() {
    let (feeds, fed) = mpsc::channel(8);
    let (send, mut sent) = mpsc::channel(8);
    let (_tuning, tuned) = watch::channel(tuning(SendMode::Continuous));
    tokio::spawn(to_chat::run(fed, send, tuned, Arc::default()));
    let silent = PcmBlock {
        start_frame: 0,
        channels: 2,
        payload: PcmPayload::Silence(FRAME + 100),
    };
    let mono = to_chat::mono(&silent);
    feeds
        .send(Feed::Audio { lane: 1, mono })
        .await
        .expect("feed");
    let frame = tokio::time::timeout(Duration::from_secs(5), sent.recv()).await;
    assert_eq!(frame.expect("no frame"), Some(vec![0.0; FRAME]));
}

#[test]
fn voice_activity_to_chat_holds_silence_back() {
    let settings = tuning(SendMode::VoiceActivity).node.to_chat;
    let mut chat = to_chat::ToChat::new(&settings);
    let meters = Meters::default();
    chat.take(Feed::Audio {
        lane: 1,
        mono: vec![0.0; 4 * FRAME],
    });
    assert_eq!(chat.next_frame(&settings, &meters), None);
    assert!(!meters.to_chat.open());
}

#[test]
fn to_chat_volume_scales_what_the_gate_lets_through() {
    let settings = VoiceGateSettings {
        mode: SendMode::Continuous,
        volume: 0.5,
        ..VoiceGateSettings::default()
    };
    let mut chat = to_chat::ToChat::new(&settings);
    chat.take(Feed::Audio {
        lane: 1,
        mono: vec![0.5; 4 * FRAME],
    });
    let frame = chat
        .next_frame(&settings, &Meters::default())
        .expect("frame");
    assert_eq!(frame, vec![0.25; FRAME]);
}

#[test]
fn to_chat_volume_never_clips() {
    let settings = VoiceGateSettings {
        mode: SendMode::Continuous,
        volume: 2.0,
        ..VoiceGateSettings::default()
    };
    assert_eq!(gate::scaled(&[0.75, -0.75], &settings), [1.0, -1.0]);
}

#[tokio::test]
async fn to_air_volume_scales_what_reaches_the_transmitter() {
    let engine = Engine::new(None);
    let links = Arc::new(FakeLinks::default());
    let mut chats = chats(&links);
    let (air, mut aired) = on_air();
    let quieter = NodeBody::VoiceChat(VoiceChatNode {
        account: ACCOUNT.to_owned(),
        channel: CHANNEL.to_owned(),
        to_air: VoiceGateSettings {
            mode: SendMode::Continuous,
            volume: 0.5,
            ..VoiceGateSettings::default()
        },
        ..VoiceChatNode::default()
    });
    settle(
        &mut chats,
        &engine,
        graph(vec![("chat", quieter)], true),
        true,
        &air,
    );
    links.heard().send(said("bob", 0.5)).await.expect("bob");
    assert_eq!(first_voice(&mut aired).await, vec![0.25; FRAME]);
}

#[test]
fn stereo_channel_audio_is_mixed_to_mono() {
    let block = PcmBlock {
        start_frame: 0,
        channels: 2,
        payload: PcmPayload::Samples(Arc::from([0.5, -0.5, 1.0, 0.0].as_slice())),
    };
    assert_eq!(to_chat::mono(&block), [0.0, 0.5]);
}

#[test]
fn an_edited_account_reopens_its_session() {
    let store = Arc::new(crate::Store::open(None).expect("store"));
    let club = store
        .create_chat_account(ChatAccount {
            id: String::new(),
            name: "Club".to_owned(),
            service: ChatService::Teamspeak {
                address: "127.0.0.1:9".to_owned(),
                password: Secret::default(),
                nickname: String::new(),
                identity: Secret::default(),
            },
        })
        .expect("create");
    let sessions = Sessions::new(store.clone());
    let link = sessions.join(&club, CHANNEL).expect("join");
    assert!(sessions.settle(std::slice::from_ref(&club)).is_empty());
    let mut moved = club.clone();
    if let ChatService::Teamspeak { address, .. } = &mut moved.service {
        "127.0.0.1:10".clone_into(address);
    }
    let moved = store.update_chat_account(&club.id, moved).expect("update");
    assert_eq!(
        sessions.settle(std::slice::from_ref(&moved)),
        std::slice::from_ref(&club.id)
    );
    drop(link);
    sessions.join(&moved, CHANNEL).expect("join again");
    assert_eq!(sessions.settle(&[]), [club.id]);
}

#[test]
fn the_audio_out_hears_every_talker_without_a_transmitter() {
    let mut to_air = to_air::ToAir::new(&VoiceGateSettings::default());
    let tuning = Tuning {
        node: VoiceChatNode::default(),
        keying: false,
    };
    let (air, mut aired) = on_air();
    let meters = Meters::default();
    let now = tokio::time::Instant::now();
    to_air.hear(said("bob", 0.25), &tuning, now, &air, &meters);
    let (tx, rx) = std::sync::mpsc::channel();
    let out: to_air::Said = Arc::new(move |pcm: &[f32]| {
        tx.send(pcm.to_vec()).expect("listening");
    });
    to_air.pass_on(&out);
    assert_eq!(rx.try_recv().expect("heard"), vec![0.25; FRAME]);
    assert!(
        aired.try_recv().is_err(),
        "nothing keys without a transmitter"
    );
}

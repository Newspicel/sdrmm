use futures::{SinkExt, StreamExt};
use livekit_protocol::{
    JoinResponse, ParticipantInfo, ParticipantUpdate, SignalRequest, SignalResponse, TrackInfo,
    participant_info::State,
};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message as Frame;

use super::*;

const CHROME_CANDIDATE: &str = "candidate:842163049 1 udp 1677729535 203.0.113.7 46154 typ srflx raddr 10.0.0.2 rport 46154 generation 0 ufrag abcd network-id 1";

async fn listen() -> (TcpListener, Url) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let url = Url::parse(&format!("ws://127.0.0.1:{port}/rtc")).expect("url");
    (listener, url)
}

async fn accept(listener: &TcpListener) -> WebSocketStream<TcpStream> {
    let (stream, _) = listener.accept().await.expect("accept");
    tokio_tungstenite::accept_async(stream)
        .await
        .expect("handshake")
}

fn frame(message: signal_response::Message) -> Frame {
    let response = SignalResponse {
        message: Some(message),
    };
    Frame::Binary(response.encode_to_vec().into())
}

fn join_frame(ping_interval: i32, ping_timeout: i32) -> Frame {
    frame(signal_response::Message::Join(JoinResponse {
        server_version: "1.9".to_owned(),
        ping_interval,
        ping_timeout,
        ..Default::default()
    }))
}

fn request_of(frame: Frame) -> signal_request::Message {
    let Frame::Binary(bytes) = frame else {
        panic!("not binary");
    };
    SignalRequest::decode(bytes.as_ref())
        .expect("decode")
        .message
        .expect("message")
}

fn participant(sid: &str, identity: &str, state: State, tracks: &[&str]) -> ParticipantInfo {
    ParticipantInfo {
        sid: sid.to_owned(),
        identity: identity.to_owned(),
        state: state as i32,
        tracks: tracks
            .iter()
            .map(|sid| TrackInfo {
                sid: (*sid).to_owned(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

fn update(participants: Vec<ParticipantInfo>) -> ParticipantUpdate {
    ParticipantUpdate { participants }
}

#[test]
fn url_switches_https_to_wss() {
    let grant = LiveKitGrant {
        url: "https://sfu.example/livekit/sfu".to_owned(),
        jwt: "tok".to_owned(),
    };
    let url = rtc_url(&grant).expect("url");
    assert_eq!(url.scheme(), "wss");
    assert_eq!(url.host_str(), Some("sfu.example"));
    assert_eq!(url.path(), "/livekit/sfu/rtc");
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["access_token"], "tok");
    assert_eq!(query["auto_subscribe"], "1");
    assert_eq!(query["sdk"], "rust");
    assert_eq!(query["protocol"], PROTOCOL_VERSION.to_string());
    assert_eq!(query["adaptive_stream"], "0");
}

#[test]
fn url_keeps_a_plain_socket_and_rejects_other_schemes() {
    let plain = LiveKitGrant {
        url: "ws://sfu.example".to_owned(),
        jwt: "t".to_owned(),
    };
    assert_eq!(rtc_url(&plain).expect("url").scheme(), "ws");
    let other = LiveKitGrant {
        url: "ftp://sfu.example".to_owned(),
        jwt: "t".to_owned(),
    };
    assert!(rtc_url(&other).is_err());
}

#[tokio::test]
async fn join_reads_the_join_response() {
    let (listener, url) = listen().await;
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        socket.send(join_frame(5, 15)).await.expect("join");
        socket
    });
    let (_signal, join) = Signal::connect(&url).await.expect("connect");
    assert_eq!(join.server_version, "1.9");
    server.await.expect("server");
}

#[tokio::test]
async fn requests_and_responses_cross_the_socket() {
    let (listener, url) = listen().await;
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        socket.send(join_frame(5, 15)).await.expect("join");
        let seen = request_of(socket.next().await.expect("frame").expect("ok"));
        socket
            .send(frame(signal_response::Message::Update(update(vec![]))))
            .await
            .expect("update");
        seen
    });
    let (mut signal, _) = Signal::connect(&url).await.expect("connect");
    signal
        .send(signal_request::Message::AddTrack(add_mic(true)))
        .await
        .expect("send");
    let message = signal.next().await.expect("next");
    assert!(matches!(message, signal_response::Message::Update(_)));
    let signal_request::Message::AddTrack(track) = server.await.expect("server") else {
        panic!("not add track");
    };
    assert_eq!(track.cid, "mic");
}

#[tokio::test]
async fn pings_go_out_and_a_silent_server_fails() {
    let (listener, url) = listen().await;
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        socket.send(join_frame(1, 2)).await.expect("join");
        let mut pings = 0;
        while let Some(Ok(frame)) = socket.next().await {
            if let Frame::Binary(_) = frame {
                pings += 1;
            }
        }
        pings
    });
    let (mut signal, _) = Signal::connect(&url).await.expect("connect");
    tokio::time::pause();
    let error = signal.next().await.expect_err("timeout");
    assert_eq!(error, "Call timed out");
    drop(signal);
    assert!(server.await.expect("server") >= 1);
}

#[tokio::test]
async fn a_pong_keeps_the_socket_alive() {
    let (listener, url) = listen().await;
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        socket.send(join_frame(1, 1)).await.expect("join");
        for _ in 0..2 {
            let ping = request_of(socket.next().await.expect("frame").expect("ok"));
            assert!(matches!(ping, signal_request::Message::PingReq(_)));
            socket
                .send(frame(signal_response::Message::PongResp(
                    livekit_protocol::Pong::default(),
                )))
                .await
                .expect("pong");
        }
        socket
            .send(frame(signal_response::Message::Update(update(vec![]))))
            .await
            .expect("update");
        socket
    });
    let (mut signal, _) = Signal::connect(&url).await.expect("connect");
    let message = signal.next().await.expect("alive");
    assert!(matches!(message, signal_response::Message::Update(_)));
    server.await.expect("server");
}

#[test]
fn roster_maps_tracks_to_identities() {
    let mut roster = Roster::default();
    roster.apply(&update(vec![
        participant("PA_1", "@a:hs:DEV", State::Active, &["TR_1"]),
        participant("PA_2", "@b:hs:DEV", State::Joined, &["TR_2", "TR_3"]),
    ]));
    assert_eq!(roster.identity_of_track("TR_1"), Some("@a:hs:DEV"));
    assert_eq!(roster.identity_of_track("TR_3"), Some("@b:hs:DEV"));
    assert_eq!(roster.identity_of_track("TR_9"), None);
    roster.apply(&update(vec![participant(
        "PA_2",
        "@b:hs:DEV",
        State::Active,
        &["TR_4"],
    )]));
    assert_eq!(roster.identity_of_track("TR_2"), None);
    assert_eq!(roster.identity_of_track("TR_4"), Some("@b:hs:DEV"));
}

#[test]
fn roster_reports_joins_and_leaves() {
    let mut roster = Roster::default();
    roster.apply(&update(vec![participant(
        "PA_1",
        "@a:hs:DEV",
        State::Joined,
        &[],
    )]));
    roster.apply(&update(vec![participant(
        "PA_1",
        "@a:hs:DEV",
        State::Active,
        &["TR_1"],
    )]));
    assert_eq!(roster.joined(), vec!["@a:hs:DEV".to_owned()]);
    assert!(roster.joined().is_empty());
    roster.apply(&update(vec![participant(
        "PA_1",
        "@a:hs:DEV",
        State::Disconnected,
        &[],
    )]));
    roster.apply(&update(vec![participant(
        "PA_7",
        "@z:hs:DEV",
        State::Disconnected,
        &[],
    )]));
    assert_eq!(roster.left(), vec!["@a:hs:DEV".to_owned()]);
    assert!(roster.left().is_empty());
    assert_eq!(roster.identity_of_track("TR_1"), None);
}

#[test]
fn the_microphone_track_asks_for_the_right_encryption() {
    let sealed = add_mic(true);
    assert_eq!(
        (sealed.cid.as_str(), sealed.name.as_str()),
        ("mic", "microphone")
    );
    assert_eq!(sealed.r#type, TrackType::Audio as i32);
    assert_eq!(sealed.source, TrackSource::Microphone as i32);
    assert_eq!(sealed.encryption, encryption::Type::Gcm as i32);
    assert_eq!(add_mic(false).encryption, encryption::Type::None as i32);
}

#[test]
fn leaving_is_client_initiated_without_reconnect() {
    let leave = leave();
    assert!(!leave.can_reconnect);
    assert_eq!(leave.reason, DisconnectReason::ClientInitiated as i32);
    assert_eq!(leave.action, leave_request::Action::Disconnect as i32);
}

#[test]
fn trickle_carries_a_candidate_both_ways() {
    let candidate = Candidate::from_sdp_string(CHROME_CANDIDATE).expect("candidate");
    let request = trickle_for(&candidate, SignalTarget::Publisher);
    assert_eq!(request.target, SignalTarget::Publisher as i32);
    assert!(!request.r#final);
    let json: serde_json::Value = serde_json::from_str(&request.candidate_init).expect("json");
    assert_eq!(json["sdpMLineIndex"], 0);
    assert!(
        json["candidate"]
            .as_str()
            .expect("text")
            .starts_with("candidate:")
    );
    let back = candidate_of(&request).expect("parse").expect("some");
    assert_eq!(back.addr(), candidate.addr());
}

#[test]
fn trickle_without_a_candidate_is_the_end_marker() {
    let request = TrickleRequest {
        candidate_init: r#"{"candidate":"","sdpMid":"0","sdpMLineIndex":0}"#.to_owned(),
        ..Default::default()
    };
    assert!(candidate_of(&request).expect("parse").is_none());
    let broken = TrickleRequest {
        candidate_init: "nope".to_owned(),
        ..Default::default()
    };
    assert!(candidate_of(&broken).is_err());
}

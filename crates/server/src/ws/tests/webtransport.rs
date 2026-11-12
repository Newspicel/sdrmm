use sdrmm_wire::{WEBTRANSPORT_LENGTH_BYTES, WEBTRANSPORT_PATH};
use wtransport::{ClientConfig, Connection, Endpoint, RecvStream, SendStream, tls::Sha256Digest};

use super::*;

struct Session {
    connection: Connection,
    commands: SendStream,
    events: RecvStream,
}

async fn listening(state: &AppState) -> sdrmm_wire::WebTransportInfo {
    crate::ws::webtransport::start(state, "127.0.0.1:0".parse().expect("address")).expect("starts");
    state.webtransport.current().expect("advertised")
}

fn pin(info: &sdrmm_wire::WebTransportInfo) -> Sha256Digest {
    let bytes = sdrmm_wire::phone::unhex(&info.certificate_sha256).expect("hex");
    Sha256Digest::new(bytes.try_into().expect("32 bytes"))
}

async fn dial_session(
    info: &sdrmm_wire::WebTransportInfo,
    path: &str,
) -> Result<Connection, wtransport::error::ConnectingError> {
    let config = ClientConfig::builder()
        .with_bind_default()
        .with_server_certificate_hashes([pin(info)])
        .build();
    Endpoint::client(config)
        .expect("client")
        .connect(format!("https://127.0.0.1:{}{path}", info.port))
        .await
}

async fn open(state: &AppState) -> Session {
    let info = listening(state).await;
    let connection = dial_session(&info, WEBTRANSPORT_PATH)
        .await
        .expect("session");
    let (commands, events) = connection
        .open_bi()
        .await
        .expect("stream credit")
        .await
        .expect("control stream");
    Session {
        connection,
        commands,
        events,
    }
}

async fn fresh_state() -> AppState {
    let store = Arc::new(crate::Store::open(None).expect("in-memory store"));
    let state = AppState::new(test_engine(), store);
    let (_, background) = crate::router_with_state(state.clone(), &crate::ServerOptions::default());
    background.detach();
    state
}

async fn read_message(stream: &mut RecvStream) -> Vec<u8> {
    let mut length = [0u8; WEBTRANSPORT_LENGTH_BYTES];
    timeout(WAIT, stream.read_exact(&mut length))
        .await
        .expect("a message in time")
        .expect("length");
    let mut message = vec![0; u32::from_le_bytes(length) as usize];
    timeout(WAIT, stream.read_exact(&mut message))
        .await
        .expect("a whole message in time")
        .expect("message");
    message
}

impl Session {
    async fn send(&mut self, command: &ClientCommand) {
        let json = serde_json::to_vec(command).expect("serialize");
        let length = u32::try_from(json.len()).expect("short").to_le_bytes();
        self.commands.write_all(&length).await.expect("length");
        self.commands.write_all(&json).await.expect("command");
    }

    async fn event(&mut self) -> ServerEvent {
        serde_json::from_slice(&read_message(&mut self.events).await).expect("event json")
    }

    async fn port_stream(&mut self) -> (u16, StreamKind) {
        loop {
            match self.event().await {
                ServerEvent::PortStream {
                    stream_id, kind, ..
                } => return (stream_id, kind),
                ServerEvent::PortRefused { reason, .. } => panic!("refused: {reason}"),
                _ => {}
            }
        }
    }

    async fn subscribe(&mut self, node: &str, port: &str) -> (u16, StreamKind) {
        self.send(&ClientCommand::SubscribePort {
            node: node.to_owned(),
            port: port.to_owned(),
            params: sdrmm_wire::StreamParams::default(),
        })
        .await;
        self.port_stream().await
    }
}

fn header(frame: &[u8]) -> (u8, u16) {
    (frame[1], u16::from_le_bytes([frame[2], frame[3]]))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_greets_on_its_control_stream() {
    let state = fresh_state().await;
    let mut session = open(&state).await;
    assert_eq!(
        session.event().await,
        ServerEvent::Hello {
            revision: state.engine.snapshot().revision,
            protocol: sdrmm_wire::API_PROTOCOL,
            phone: None,
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn spectrum_frames_arrive_on_a_lane_of_their_own() {
    let state = fresh_state().await;
    workspace_on(
        &state,
        graph(
            vec![radio("band"), of("scope", sdrmm_wire::NodeKind::Scope)],
            vec![wire(("radio", "iq"), ("scope", "iq"))],
        ),
    )
    .await;
    let mut session = open(&state).await;
    let (stream_id, kind) = session.subscribe("scope", "iq").await;
    assert_eq!(kind, StreamKind::Spectrum);

    let mut lane = timeout(WAIT, session.connection.accept_uni())
        .await
        .expect("a lane in time")
        .expect("lane");
    for _ in 0..2 {
        let frame = read_message(&mut lane).await;
        assert_eq!(
            header(&frame),
            (sdrmm_wire::FrameKind::Spectrum as u8, stream_id)
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn audio_arrives_as_datagrams() {
    let state = fresh_state().await;
    workspace_on(
        &state,
        graph(
            vec![
                radio("band"),
                channel("voice", "nfm"),
                of("spk", sdrmm_wire::NodeKind::Speaker),
            ],
            vec![
                wire(("radio", "iq"), ("voice", "iq")),
                wire(("voice", "audio"), ("spk", "audio")),
            ],
        ),
    )
    .await;
    let mut session = open(&state).await;
    let (stream_id, kind) = session.subscribe("spk", "audio").await;
    assert_eq!(kind, StreamKind::Audio);

    let datagram = timeout(WAIT, session.connection.receive_datagram())
        .await
        .expect("audio in time")
        .expect("datagram");
    assert_eq!(
        header(&datagram.payload()),
        (sdrmm_wire::FrameKind::AudioOpus as u8, stream_id)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_counts_as_a_client_until_it_leaves() {
    let state = fresh_state().await;
    let mut session = open(&state).await;
    session.event().await;
    assert_eq!(state.clients.load(atomic::Ordering::Relaxed), 1);

    session
        .connection
        .close(wtransport::VarInt::from_u32(0), b"");
    let deadline = Instant::now() + WAIT;
    while state.clients.load(atomic::Ordering::Relaxed) != 0 {
        assert!(Instant::now() < deadline, "the session never ended");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_session_path_is_taken() {
    let state = fresh_state().await;
    let info = listening(&state).await;
    assert!(dial_session(&info, "/api/ws").await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_token_is_asked_of_a_session_too() {
    let mut state = fresh_state().await;
    state.auth = crate::auth::Auth::new(Some("s3cret"));
    let info = listening(&state).await;

    assert!(dial_session(&info, WEBTRANSPORT_PATH).await.is_err());
    let wrong = format!("{WEBTRANSPORT_PATH}?token=nope");
    assert!(dial_session(&info, &wrong).await.is_err());
    let right = format!("{WEBTRANSPORT_PATH}?token=s3cret");
    assert!(dial_session(&info, &right).await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_pinned_certificate_is_what_a_browser_accepts_by_hash() {
    let state = fresh_state().await;
    let info = listening(&state).await;
    assert_eq!(info.certificate_sha256.len(), 64);
    let mut other = info.clone();
    other.certificate_sha256 = "00".repeat(32);
    assert!(dial_session(&other, WEBTRANSPORT_PATH).await.is_err());
    assert!(dial_session(&info, WEBTRANSPORT_PATH).await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_reports_the_health_of_its_path() {
    let state = fresh_state().await;
    let mut session = open(&state).await;
    session
        .send(&ClientCommand::SubscribeDiagnostics { enabled: true })
        .await;
    let link = loop {
        if let ServerEvent::PipelineHealth { link, .. } = session.event().await {
            break link.expect("a measured path");
        }
    };
    assert!(link.rtt_ms > 0.0 && link.rtt_ms < 1000.0, "{link:?}");
    assert_eq!(link.loss, 0.0);
}

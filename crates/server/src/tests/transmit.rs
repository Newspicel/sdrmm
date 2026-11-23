use std::time::{Duration, Instant};

use sdrmm_wire::{
    AudioProcessing, EncoderNode, MicrophoneNode, NodeActed, NodeBody, PatchEdge, PatchNode,
    PortRef, Position, RadioActivity, RadioUse, TransmitNode, TransmitPhase, TransmitSettings,
    TransmitStatus, WorkspaceSnapshot,
};

use super::*;

fn node(id: &str, body: NodeBody) -> PatchNode {
    PatchNode {
        id: id.to_owned(),
        body,
        position: Position { x: 600.0, y: 300.0 },
        size: None,
        label: None,
    }
}

fn wire(from: (&str, &str), to: (&str, &str)) -> PatchEdge {
    let end = |(node, port): (&str, &str)| PortRef {
        node: node.to_owned(),
        port: port.to_owned(),
    };
    PatchEdge {
        from: end(from),
        to: end(to),
    }
}

fn encoder(encoder_type: &str) -> NodeBody {
    NodeBody::Encoder(EncoderNode {
        encoder_type: encoder_type.to_owned(),
        settings: serde_json::Value::Null,
        audio: Default::default(),
    })
}

/// Sets the bench radio to `radio_use`, as its Device node's use setting does.
fn used_for(snapshot: &mut WorkspaceSnapshot, radio_use: RadioUse) {
    for held in &mut snapshot.graph.nodes {
        if let NodeBody::Device(device) = &mut held.body {
            device.radio_use = radio_use;
        }
    }
}

fn station(encoder_type: &str) -> WorkspaceSnapshot {
    let mut snapshot = virtual_snapshot("transceiver", &[]);
    used_for(&mut snapshot, RadioUse::Both);
    snapshot.graph.nodes.extend([
        node("mic", NodeBody::Microphone(MicrophoneNode::default())),
        node("mod", encoder(encoder_type)),
        node("tx", NodeBody::Transmit(TransmitNode::default())),
    ]);
    snapshot.graph.edges.extend([
        wire(("mic", "audio"), ("mod", "audio")),
        wire(("mod", "baseband"), ("tx", "baseband")),
        wire(("tx", "transmit"), ("device", "transmit")),
    ]);
    snapshot
}

async fn on_the_bench(app: &Router, snapshot: &WorkspaceSnapshot) {
    let workspace = put_active_workspace(app, snapshot).await;
    apply(app, workspace).await;
}

async fn act(app: &Router, node: &str, action: serde_json::Value) -> (StatusCode, String) {
    let path = format!("/api/workspaces/active/nodes/{node}/actions");
    let (status, body) = request(app.clone(), "POST", &path, Some(&action.to_string())).await;
    (status, String::from_utf8_lossy(&body).into_owned())
}

async fn acted(app: &Router, node: &str, action: serde_json::Value) -> NodeActed {
    let (status, body) = act(app, node, action).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    serde_json::from_str(&body).expect("json")
}

fn arm() -> serde_json::Value {
    serde_json::json!({"action": "transmit", "with": {"do": "arm"}})
}

fn disarm() -> serde_json::Value {
    serde_json::json!({"action": "transmit", "with": {"do": "disarm"}})
}

fn send(message: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({"action": "send", "with": {"message": message}})
}

async fn transmitting(app: &Router) -> Option<TransmitStatus> {
    get_state(app)
        .await
        .device_sets
        .into_iter()
        .find_map(|set| set.transmit)
}

async fn until(app: &Router, what: &str, done: impl Fn(&TransmitStatus) -> bool) -> TransmitStatus {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = transmitting(app).await
            && done(&status)
        {
            return status;
        }
        assert!(Instant::now() < deadline, "never {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn an_armed_transmitter_sends_the_message_its_encoder_is_handed() {
    let app = test_router();
    on_the_bench(&app, &station("aprs")).await;
    assert_eq!(transmitting(&app).await, None, "nothing transmits unasked");

    let hello = send(&serde_json::json!({"source": "N0CALL-7", "text": ">hello"}));
    assert_eq!(acted(&app, "mod", hello.clone()).await, NodeActed::Sent);
    assert_eq!(
        transmitting(&app).await,
        None,
        "an unarmed transmitter stays quiet"
    );

    let NodeActed::Armed(armed) = acted(&app, "tx", arm()).await else {
        panic!("arming answers with the status");
    };
    assert_eq!(armed.node, "tx");
    assert_eq!(armed.phase, TransmitPhase::Idle);
    assert_eq!(acted(&app, "mod", hello).await, NodeActed::Sent);
    let sent = until(&app, "sent the burst", |status| {
        status.bursts == 1 && status.phase == TransmitPhase::Idle
    })
    .await;
    assert!(sent.samples > 0);
    assert_eq!(sent.error, None);

    let (status, body) = act(&app, "mod", send(&serde_json::json!({"text": "x"}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("source"), "{body}");

    assert_eq!(acted(&app, "tx", disarm()).await, NodeActed::Disarmed);
    assert_eq!(transmitting(&app).await, None);
    let (status, body) = act(&app, "tx", disarm()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("Not armed"), "{body}");
}

#[tokio::test]
async fn a_transmitter_is_refused_until_its_wiring_can_carry_a_signal() {
    let app = test_router();
    let mut loose = station("nfm");
    loose
        .graph
        .edges
        .retain(|edge| edge.to.port != "transmit" && edge.to.port != "baseband");
    on_the_bench(&app, &loose).await;
    let (status, body) = act(&app, "tx", arm()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("radio's transmit input"), "{body}");

    loose
        .graph
        .edges
        .push(wire(("tx", "transmit"), ("device", "transmit")));
    on_the_bench(&app, &loose).await;
    let (_, body) = act(&app, "tx", arm()).await;
    assert!(
        body.contains("Wire an encoder, a channel or a lane"),
        "{body}"
    );

    on_the_bench(&app, &station("")).await;
    let (_, body) = act(&app, "tx", arm()).await;
    assert!(body.contains("Pick the encoder's mode"), "{body}");

    let (status, body) = act(&app, "mod", arm()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("not a transmitter"), "{body}");
    let (status, _) = act(&app, "nobody", arm()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let mut deaf = virtual_snapshot("quad", &[]);
    deaf.graph.nodes.extend([
        node("mod", encoder("nfm")),
        node("tx", NodeBody::Transmit(TransmitNode::default())),
    ]);
    deaf.graph.edges.extend([
        wire(("mod", "baseband"), ("tx", "baseband")),
        wire(("tx", "transmit"), ("device", "transmit")),
    ]);
    on_the_bench(&app, &deaf).await;
    let (status, body) = act(&app, "tx", arm()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("does not transmit"), "{body}");
}

#[tokio::test]
async fn an_armed_transmitter_follows_the_patch() {
    let (app, state) = test_router_with_state();
    let mut snapshot = station("nfm");
    on_the_bench(&app, &snapshot).await;
    acted(&app, "tx", arm()).await;
    let spoken = crate::encoders::speak(&state, "mic", &vec![0.1; 4_800]);
    assert_eq!(spoken, 1, "the microphone reaches its transmitter");
    until(&app, "keyed on voice", |status| status.bursts == 1).await;

    let quieter = TransmitSettings {
        level_db: -12.0,
        ..TransmitSettings::default()
    };
    for held in &mut snapshot.graph.nodes {
        if let NodeBody::Transmit(body) = &mut held.body {
            body.settings = quieter.clone();
        }
    }
    put_active_workspace(&app, &snapshot).await;
    let followed = until(&app, "followed the saved level", |status| {
        status.settings == quieter
    })
    .await;
    assert_eq!(followed.bursts, 1, "a level change keeps it armed");

    snapshot
        .graph
        .edges
        .retain(|edge| edge.to.port != "transmit");
    put_active_workspace(&app, &snapshot).await;
    let deadline = Instant::now() + Duration::from_secs(20);
    while transmitting(&app).await.is_some() {
        assert!(
            Instant::now() < deadline,
            "an unwired transmitter stayed armed"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        crate::encoders::speak(&state, "mic", &[0.0; 480]),
        1,
        "the encoder still feeds the transmitter wired to it"
    );
    snapshot
        .graph
        .edges
        .retain(|edge| edge.to.port != "baseband");
    put_active_workspace(&app, &snapshot).await;
    let deadline = Instant::now() + Duration::from_secs(20);
    while state.engine.runs_encoder("mod") {
        assert!(Instant::now() < deadline, "an unwired encoder kept running");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(crate::encoders::speak(&state, "mic", &[0.0; 480]), 0);
}

#[tokio::test]
async fn an_encoders_audio_settings_follow_into_the_running_encoder() {
    let (app, state) = test_router_with_state();
    let mut snapshot = station("nfm");
    on_the_bench(&app, &snapshot).await;
    acted(&app, "tx", arm()).await;
    let notched = AudioProcessing {
        auto_notch: true,
        ..AudioProcessing::default()
    };
    for held in &mut snapshot.graph.nodes {
        if let NodeBody::Encoder(body) = &mut held.body {
            body.audio = notched.clone();
        }
    }
    put_active_workspace(&app, &snapshot).await;
    let deadline = Instant::now() + Duration::from_secs(20);
    while state
        .engine
        .encoder_spec("mod")
        .is_none_or(|spec| spec.audio != notched)
    {
        assert!(
            Instant::now() < deadline,
            "the audio settings never arrived"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(crate::encoders::speak(&state, "mic", &vec![0.1; 4_800]), 1);
    let keyed = until(&app, "keyed through the chain", |status| status.bursts == 1).await;
    assert_eq!(keyed.error, None);
}

#[tokio::test]
async fn a_radio_set_to_receive_keeps_its_transmitter_off_the_air() {
    let app = test_router();
    let mut snapshot = station("nfm");
    used_for(&mut snapshot, RadioUse::Receive);
    on_the_bench(&app, &snapshot).await;
    let (status, body) = act(&app, "tx", arm()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains(sdrmm_engine::SET_TO_RECEIVE), "{body}");

    used_for(&mut snapshot, RadioUse::Both);
    put_active_workspace(&app, &snapshot).await;
    acted(&app, "tx", arm()).await;

    used_for(&mut snapshot, RadioUse::Send);
    put_active_workspace(&app, &snapshot).await;
    let radio = get_state(&app).await.device_sets.remove(0);
    assert_eq!(radio.radio_use, RadioUse::Send);
    assert_eq!(
        radio.activity,
        RadioActivity::Idle,
        "armed, not keyed, not listening"
    );

    used_for(&mut snapshot, RadioUse::Receive);
    put_active_workspace(&app, &snapshot).await;
    assert_eq!(
        transmitting(&app).await,
        None,
        "receive takes it off the air"
    );
}

#[tokio::test]
async fn a_channels_audio_keys_the_transmitter_it_is_wired_to() {
    channel_keys(AudioProcessing::default()).await;
}

#[tokio::test]
async fn a_channels_processed_audio_keys_the_transmitter_it_is_wired_to() {
    channel_keys(AudioProcessing {
        auto_notch: true,
        ..AudioProcessing::default()
    })
    .await;
}

async fn channel_keys(audio: AudioProcessing) {
    let app = test_router();
    let mut snapshot = virtual_snapshot("transceiver", &[("ear", "nfm", "iq")]);
    used_for(&mut snapshot, RadioUse::Both);
    for held in &mut snapshot.graph.nodes {
        if let NodeBody::Channel(channel) = &mut held.body {
            channel.audio = audio.clone();
        }
    }
    snapshot.graph.nodes.extend([
        node("mod", encoder("nfm")),
        node("tx", NodeBody::Transmit(TransmitNode::default())),
    ]);
    snapshot.graph.edges.extend([
        wire(("ear", "audio"), ("mod", "audio")),
        wire(("mod", "baseband"), ("tx", "baseband")),
        wire(("tx", "transmit"), ("device", "transmit")),
    ]);
    on_the_bench(&app, &snapshot).await;
    acted(&app, "tx", arm()).await;
    let keyed = until(&app, "keyed by the channel", |status| status.bursts >= 1).await;
    assert_eq!(keyed.error, None);
    assert_eq!(acted(&app, "tx", disarm()).await, NodeActed::Disarmed);
}

#[tokio::test]
async fn a_channels_baseband_wired_to_a_transmitter_is_what_it_sends() {
    let app = test_router();
    let mut snapshot = virtual_snapshot("transceiver", &[("ear", "nfm", "iq")]);
    used_for(&mut snapshot, RadioUse::Both);
    snapshot
        .graph
        .nodes
        .push(node("tx", NodeBody::Transmit(TransmitNode::default())));
    snapshot.graph.edges.extend([
        wire(("ear", "baseband"), ("tx", "baseband")),
        wire(("tx", "transmit"), ("device", "transmit")),
    ]);
    on_the_bench(&app, &snapshot).await;
    acted(&app, "tx", arm()).await;
    let sending = until(&app, "sent the channel", |status| {
        status.phase == TransmitPhase::OnAir
    })
    .await;
    assert_eq!(sending.error, None);
    assert_eq!(acted(&app, "tx", disarm()).await, NodeActed::Disarmed);
}

fn microphone_send(snapshot: &mut WorkspaceSnapshot, send: sdrmm_wire::SendMode) {
    for held in &mut snapshot.graph.nodes {
        if let NodeBody::Microphone(body) = &mut held.body {
            body.send = send;
        }
    }
}

#[tokio::test]
async fn voice_activity_microphone_drops_quiet_frames() {
    let (app, state) = test_router_with_state();
    let mut snapshot = station("nfm");
    microphone_send(&mut snapshot, sdrmm_wire::SendMode::VoiceActivity);
    on_the_bench(&app, &snapshot).await;
    acted(&app, "tx", arm()).await;
    assert_eq!(
        crate::encoders::speak(&state, "mic", &vec![0.0005; 4_800]),
        0
    );
}

/// A loud vowel-like buzz: a 120 Hz pulse train through two formants, as speech scores.
fn voiced(samples: usize) -> Vec<f32> {
    let rate = 48_000.0_f32;
    let formant = |hz: f32| std::f32::consts::TAU * hz / rate;
    let (w1, w2) = (formant(730.0), formant(1_090.0));
    let (mut a, mut av, mut b, mut bv, mut phase) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
    let raw: Vec<f32> = (0..samples)
        .map(|i| {
            let last = phase;
            phase = (phase + (120.0 + 30.0 * (i as f32 / rate * 1.3).sin()) / rate).fract();
            let pulse = f32::from(phase < last);
            av = av * 0.993 - w1 * a + pulse;
            a += w1 * av;
            bv = bv * 0.993 - w2 * b + pulse;
            b += w2 * bv;
            a + 0.5 * b
        })
        .collect();
    let peak = raw
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    raw.iter().map(|sample| sample * 0.5 / peak).collect()
}

#[tokio::test]
async fn voice_activity_microphone_opens_on_a_loud_voice() {
    let (app, state) = test_router_with_state();
    let mut snapshot = station("nfm");
    microphone_send(&mut snapshot, sdrmm_wire::SendMode::VoiceActivity);
    on_the_bench(&app, &snapshot).await;
    acted(&app, "tx", arm()).await;
    let taken: usize = voiced(100 * 960)
        .chunks(960)
        .map(|frame| crate::encoders::speak(&state, "mic", frame))
        .sum();
    assert!(taken > 0, "a loud voice never opened the microphone");
}

#[tokio::test]
async fn continuous_microphone_passes_quiet_frames() {
    let (app, state) = test_router_with_state();
    let mut snapshot = station("nfm");
    microphone_send(&mut snapshot, sdrmm_wire::SendMode::Continuous);
    on_the_bench(&app, &snapshot).await;
    acted(&app, "tx", arm()).await;
    assert_eq!(
        crate::encoders::speak(&state, "mic", &vec![0.0005; 4_800]),
        1
    );
}

#[tokio::test]
async fn a_source_feeds_an_encoder_while_the_encoder_is_wired() {
    let (app, state) = test_router_with_state();
    let mut snapshot = station("nfm");
    on_the_bench(&app, &snapshot).await;
    let feeds = || crate::encoders::feeds_encoder(&state.engine, &state.resolved.current(), "mic");
    assert!(feeds(), "a wired encoder runs armed or not");
    assert!(!crate::encoders::feeds_encoder(
        &state.engine,
        &state.resolved.current(),
        "mod"
    ));
    let mut changes = state.encoders.subscribe_changes();
    changes.mark_unchanged();
    snapshot
        .graph
        .edges
        .retain(|edge| edge.to.port != "baseband");
    put_active_workspace(&app, &snapshot).await;
    let deadline = Instant::now() + Duration::from_secs(20);
    while feeds() {
        assert!(
            Instant::now() < deadline,
            "the unwired encoder kept running"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        changes.has_changed().expect("hub"),
        "an encoder stopping wakes who feeds it"
    );
}

/// An encoder wired only to a recorder, its microphone wired only to another.
fn studio() -> WorkspaceSnapshot {
    let mut snapshot = virtual_snapshot("transceiver", &[]);
    snapshot.graph.nodes.extend([
        node("mic", NodeBody::Microphone(MicrophoneNode::default())),
        node("mod", encoder("nfm")),
        node(
            "base",
            NodeBody::BasebandRecorder(sdrmm_wire::BasebandRecorderNode { recording: true }),
        ),
        node(
            "takes",
            NodeBody::AudioRecorder(sdrmm_wire::AudioRecorderNode {
                recording: true,
                ..Default::default()
            }),
        ),
    ]);
    snapshot.graph.edges.extend([
        wire(("mic", "audio"), ("mod", "audio")),
        wire(("mic", "audio"), ("takes", "audio")),
        wire(("mod", "baseband"), ("base", "baseband")),
    ]);
    snapshot
}

#[tokio::test]
async fn an_encoder_and_a_microphone_record_with_no_radio_in_the_way() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let (app, state) = recording_router_with_state(dir.path());
    on_the_bench(&app, &studio()).await;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let streams = get_state(&app).await.node_streams;
        let encoded = streams
            .iter()
            .any(|stream| stream.node == "mod" && stream.baseband_recording.is_some());
        let voiced = streams
            .iter()
            .any(|stream| stream.node == "mic" && !stream.audio_recordings.is_empty());
        if encoded && voiced {
            break;
        }
        assert!(Instant::now() < deadline, "never recorded: {streams:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(crate::encoders::speak(&state, "mic", &vec![0.1; 4_800]), 1);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let streams = get_state(&app).await.node_streams;
        let frames = streams
            .iter()
            .flat_map(|stream| &stream.audio_recordings)
            .map(|take| take.frames)
            .sum::<u64>();
        if frames >= 4_800 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the voice never reached its take"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

use std::time::{Duration, Instant};

use sdrmm_wire::{
    MicrophoneNode, ModulatorNode, NodeActed, NodeBody, PatchEdge, PatchNode, PortRef, Position,
    TransmitNode, TransmitPhase, TransmitSettings, TransmitStatus, WorkspaceSnapshot,
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

fn modulator(channel_type: &str) -> NodeBody {
    NodeBody::Modulator(ModulatorNode {
        channel_type: channel_type.to_owned(),
        settings: serde_json::Value::Null,
    })
}

fn station(channel_type: &str) -> WorkspaceSnapshot {
    let mut snapshot = virtual_snapshot("transceiver", &[]);
    snapshot.graph.nodes.extend([
        node("mic", NodeBody::Microphone(MicrophoneNode::default())),
        node("mod", modulator(channel_type)),
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

fn send(message: serde_json::Value) -> serde_json::Value {
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
async fn an_armed_transmitter_sends_the_message_its_modulator_is_handed() {
    let app = test_router();
    on_the_bench(&app, &station("aprs")).await;
    assert_eq!(transmitting(&app).await, None, "nothing transmits unasked");

    let hello = send(serde_json::json!({"source": "N0CALL-7", "text": ">hello"}));
    let (status, body) = act(&app, "mod", hello.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("Arm the transmitter"), "{body}");

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

    let (status, body) = act(&app, "mod", send(serde_json::json!({"text": "x"}))).await;
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
        body.contains("Wire a modulator, a channel or a lane"),
        "{body}"
    );

    on_the_bench(&app, &station("")).await;
    let (_, body) = act(&app, "tx", arm()).await;
    assert!(body.contains("Pick the modulator's mode"), "{body}");

    let (status, body) = act(&app, "mod", arm()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("not a transmitter"), "{body}");
    let (status, _) = act(&app, "nobody", arm()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let mut deaf = virtual_snapshot("quad", &[]);
    deaf.graph.nodes.extend([
        node("mod", modulator("nfm")),
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
    let spoken = crate::transmit::speak(&state, "mic", &vec![0.1; 4_800]);
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
    assert_eq!(crate::transmit::speak(&state, "mic", &[0.0; 480]), 0);
}

#[tokio::test]
async fn a_channels_audio_keys_the_transmitter_it_is_wired_to() {
    let app = test_router();
    let mut snapshot = virtual_snapshot("transceiver", &[("ear", "nfm", "iq")]);
    snapshot.graph.nodes.extend([
        node("mod", modulator("nfm")),
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

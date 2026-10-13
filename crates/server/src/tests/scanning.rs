use sdrmm_wire::{ControlAck, NodeActed, NodeBody, SweepStatus};

use super::*;

async fn rig(app: &Router, tool: &str, body: NodeBody) -> (u32, u32) {
    let ds = open_virtual_radio(app).await;
    let ch = open_channel(
        app,
        ds,
        "tap-nfm",
        serde_json::json!({"frequency_hz": 100_100_000.0, "params": {"type": "nfm", "settings": {}}}),
    )
    .await;
    draw(
        app,
        tool,
        body,
        &[
            [tool, "control", "tap-nfm", "control"],
            ["radio:band", "iq", tool, "iq"],
        ],
    )
    .await;
    (ds, ch)
}

async fn sweep(app: &Router, node: &str, verb: &str, action: &str) -> ControlAck {
    let (status, body) = act_on(
        app,
        node,
        serde_json::json!({"action": verb, "with": action}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let NodeActed::Controlled(mut acks) = serde_json::from_slice(&body).expect("json") else {
        panic!("a sweep acts through control");
    };
    assert_eq!(acks.len(), 1, "{acks:?}");
    acks.remove(0)
}

fn scanner(settings: sdrmm_wire::ScanSettings) -> NodeBody {
    NodeBody::Scanner(sdrmm_wire::ScannerNode { settings })
}

#[tokio::test]
async fn a_scanner_node_starts_skips_and_stops_its_decoder() {
    let app = test_router();
    let empty = sdrmm_wire::ScanSettings {
        ranges: Vec::new(),
        ..sdrmm_wire::ScanSettings::default()
    };
    rig(&app, "scan", scanner(empty)).await;

    for action in ["start", "stop", "skip"] {
        let ack = sweep(&app, "scan", "scan", action).await;
        assert!(
            ack.error.is_some(),
            "{action} with nothing to scan: {ack:?}"
        );
    }

    let workspace = workspaces(&app).await.active.expect("seeded workspace");
    let mut snapshot = workspace_detail(&app, workspace).await.snapshot;
    let settings: sdrmm_wire::ScanSettings = serde_json::from_value(serde_json::json!({
        "ranges": [{"start_hz": 99_000_000.0, "stop_hz": 101_000_000.0, "step_hz": 100_000.0}],
        "margin_db": 200.0,
        "dwell_ms": 40,
        "hardware_sweep": false,
    }))
    .expect("scan settings");
    if let Some(node) = snapshot
        .graph
        .nodes
        .iter_mut()
        .find(|node| node.id == "scan")
    {
        node.body = scanner(settings);
    }
    put_workspace(&app, &snapshot).await;

    let ack = sweep(&app, "scan", "scan", "start").await;
    assert_eq!(ack.error, None);
    let Some(SweepStatus::Scan(status)) = ack.sweep.map(|sweep| *sweep) else {
        panic!("a scan reports its status");
    };
    assert_eq!(status.targets, 21);
    assert_eq!(status.settings.node.as_deref(), Some("scan"));
    assert!(
        status.settings.measure_bw_hz.is_some(),
        "the decoder's bandwidth is reported as the one measured"
    );
    assert_eq!(get_state(&app).await.device_sets[0].scanners.len(), 1);

    let ack = sweep(&app, "scan", "scan", "skip").await;
    assert!(ack.error.is_some(), "nothing held to skip");

    let (status, body) = patch_node_device(&app, "radio:band", r#"{"center_hz":88000000.0}"#).await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "a scan never owns the dial: {}",
        String::from_utf8_lossy(&body)
    );

    assert_eq!(sweep(&app, "scan", "scan", "stop").await.error, None);
    assert!(get_state(&app).await.device_sets[0].scanners.is_empty());
}

fn hunter(mount_offset_deg: f64) -> NodeBody {
    NodeBody::Hunt(sdrmm_wire::HuntNode {
        clicks: false,
        sweep: sdrmm_wire::HuntSweepParams {
            mount_offset_deg,
            ..sdrmm_wire::HuntSweepParams::default()
        },
    })
}

#[tokio::test]
async fn a_hunt_node_starts_and_stops_its_decoder() {
    let app = test_router();
    let (_, ch) = rig(&app, "hunt", hunter(0.0)).await;

    let ack = sweep(&app, "hunt", "hunt", "stop").await;
    assert!(ack.error.is_some(), "no hunt to stop yet");

    let ack = sweep(&app, "hunt", "hunt", "start").await;
    assert_eq!(ack.error, None);
    let Some(SweepStatus::Hunt(hunt)) = ack.sweep.map(|sweep| *sweep) else {
        panic!("a hunt reports its status");
    };
    assert_eq!(hunt.settings.channel, ch);
    assert_eq!(hunt.settings.node.as_deref(), Some("hunt"));
    assert_eq!(hunt.freq_hz, 100_100_000.0, "the hunt reads the decoder");
    assert_eq!(get_state(&app).await.device_sets[0].hunts.len(), 1);

    let (status, _) = patch_node_device(&app, "radio:band", r#"{"gains":[]}"#).await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "a hunt leaves the radio to its operator"
    );

    assert_eq!(sweep(&app, "hunt", "hunt", "stop").await.error, None);
    assert!(get_state(&app).await.device_sets[0].hunts.is_empty());
}

fn phone_pose(heading_deg: Option<f64>) -> sdrmm_wire::PositionFix {
    sdrmm_wire::PositionFix {
        latitude: 48.137,
        longitude: 11.575,
        altitude_m: None,
        accuracy_m: Some(5.0),
        speed_mps: None,
        track_deg: None,
        time: jiff::Timestamp::now().to_string(),
        attitude: sdrmm_wire::Attitude {
            heading_deg,
            ..sdrmm_wire::Attitude::default()
        },
    }
}

fn now_ms() -> u64 {
    u64::try_from(jiff::Timestamp::now().as_millisecond()).expect("after 1970")
}

#[tokio::test]
async fn a_hunt_mark_needs_a_running_hunt_a_position_and_a_heading() {
    let (app, state) = test_router_with_state();
    let (ds, ch) = rig(&app, "hunt", hunter(-90.0)).await;
    let error = async |action: &str| {
        sweep(&app, "hunt", "hunt", action)
            .await
            .error
            .unwrap_or_default()
    };

    assert_eq!(error("mark").await, "Not running");
    assert_eq!(sweep(&app, "hunt", "hunt", "start").await.error, None);

    let ack = sweep(&app, "hunt", "hunt", "sweep").await;
    let Some(SweepStatus::Hunt(sweeping)) = ack.sweep.map(|sweep| *sweep) else {
        panic!("a sweep reports the hunt: {:?}", ack.error);
    };
    assert_eq!(
        sweeping.sweep.map(|sweep| sweep.state),
        Some(sdrmm_wire::SweepState::NoHeading),
        "a running hunt takes the sweep on"
    );
    assert_eq!(sweeping.settings.sweep.mount_offset_deg, -90.0);
    assert_eq!(error("mark").await, "No position");

    state
        .engine
        .hunt_pose(ds, ch, phone_pose(None), now_ms())
        .expect("pose");
    assert_eq!(error("mark").await, "No heading");

    let mut decoded = state.engine.subscribe_decoded();
    state
        .engine
        .hunt_pose(ds, ch, phone_pose(Some(100.0)), now_ms())
        .expect("pose");
    assert_eq!(sweep(&app, "hunt", "hunt", "mark").await.error, None);
    let bearing = loop {
        let record = tokio::time::timeout(std::time::Duration::from_secs(5), decoded.recv())
            .await
            .expect("mark event")
            .expect("decoded stream");
        if let DecoderEvent::Df(bearing) = record.event {
            assert_eq!(
                record.origin.map(|origin| origin.node).as_deref(),
                Some("hunt")
            );
            break bearing;
        }
    };
    assert_eq!(bearing.bearing_deg, 10.0);
    assert_eq!(bearing.source, sdrmm_wire::BearingSource::Mark);
    assert_eq!(sweep(&app, "hunt", "hunt", "stop").await.error, None);
}

#[tokio::test]
async fn a_node_action_names_a_node_of_the_active_workspace() {
    let app = test_router();
    let scan = Some(r#"{"action":"scan","with":"start"}"#);
    let (status, body) = request(
        app.clone(),
        "POST",
        "/api/workspaces/active/nodes/ghost/actions",
        scan,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let (status, body) = request(
        app.clone(),
        "POST",
        "/api/workspaces/active/nodes/ghost/actions",
        Some(r#"{"action":"fly"}"#),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        String::from_utf8_lossy(&body)
    );
}

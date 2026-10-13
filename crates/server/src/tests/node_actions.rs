use sdrmm_device::{DeviceDriver, DeviceError, SdrDevice};
use sdrmm_wire::{DeviceInfo, NodeActed, NodeBody, WorkspaceNoticeKind};

use super::*;

async fn act(app: &Router, node: &str, action: serde_json::Value) -> (StatusCode, Bytes) {
    request(
        app.clone(),
        "POST",
        &format!("/api/workspaces/active/nodes/{node}/actions"),
        Some(&action.to_string()),
    )
    .await
}

async fn acted(app: &Router, node: &str, action: serde_json::Value) -> NodeActed {
    let (status, body) = act(app, node, action).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    serde_json::from_slice(&body).expect("json")
}

fn tune(freq_hz: f64) -> serde_json::Value {
    serde_json::json!({"action": "tune", "with": {"freq_hz": freq_hz}})
}

fn lock_channel(snapshot: &mut sdrmm_wire::WorkspaceSnapshot, node: &str) {
    let Some(NodeBody::Channel(channel)) = snapshot
        .graph
        .nodes
        .iter_mut()
        .find(|held| held.id == node)
        .map(|held| &mut held.body)
    else {
        panic!("{node} is a channel");
    };
    channel.tuning_locked = true;
}

fn live_frequency(state: &StateSnapshot, channel_type: &str) -> f64 {
    state.device_sets[0]
        .channels
        .iter()
        .find(|channel| channel.settings.params.type_id() == channel_type)
        .expect("the channel runs")
        .settings
        .frequency_hz
}

#[tokio::test]
async fn tuning_a_locked_channel_is_refused() {
    let app = test_router();
    let mut snapshot = virtual_snapshot("band", &[("voice", "nfm", "iq"), ("held", "am", "iq")]);
    lock_channel(&mut snapshot, "held");
    let workspace = put_active_workspace(&app, &snapshot).await;
    apply(&app, workspace).await;
    let before = live_frequency(&get_state(&app).await, "am");

    let (status, body) = act(&app, "held", tune(before + 5_000.0)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(String::from_utf8_lossy(&body).contains("Tuning locked"));
    assert!((live_frequency(&get_state(&app).await, "am") - before).abs() < 1.0);

    let wanted = live_frequency(&get_state(&app).await, "nfm") + 12_500.0;
    assert_eq!(acted(&app, "voice", tune(wanted)).await, NodeActed::Tuned);
    assert!((live_frequency(&get_state(&app).await, "nfm") - wanted).abs() < 1.0);
}

#[tokio::test]
async fn a_decoder_on_a_control_wire_is_tuned_only_by_that_wire() {
    let app = test_router();
    let mut snapshot = virtual_snapshot("band", &[("voice", "nfm", "iq")]);
    snapshot.graph.nodes.push(sdrmm_wire::PatchNode {
        id: "scan".to_owned(),
        body: NodeBody::Scanner(sdrmm_wire::ScannerNode::default()),
        position: sdrmm_wire::Position { x: 800.0, y: 300.0 },
        size: None,
        label: None,
    });
    snapshot.graph.edges.push(sdrmm_wire::PatchEdge {
        from: sdrmm_wire::PortRef {
            node: "scan".to_owned(),
            port: "control".to_owned(),
        },
        to: sdrmm_wire::PortRef {
            node: "voice".to_owned(),
            port: "control".to_owned(),
        },
    });
    let workspace = put_active_workspace(&app, &snapshot).await;
    apply(&app, workspace).await;
    let state = get_state(&app).await;
    let before = live_frequency(&state, "nfm");

    let (status, body) = act(&app, "voice", tune(before + 5_000.0)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(String::from_utf8_lossy(&body).contains("Tuned by Scanner"));

    let mut settings = state.device_sets[0].channels[0].settings.clone();
    settings.frequency_hz = before + 5_000.0;
    let path = "/api/workspaces/active/nodes/voice/channel";
    let body = serde_json::json!({ "settings": settings }).to_string();
    let (status, _) = request(app.clone(), "PATCH", path, Some(&body)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!((live_frequency(&get_state(&app).await, "nfm") - before).abs() < 1.0);

    settings.frequency_hz = before;
    let body = serde_json::json!({ "settings": settings }).to_string();
    let (status, _) = request(app.clone(), "PATCH", path, Some(&body)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn tuning_a_locked_lane_is_refused() {
    let app = test_router();
    let mut snapshot = virtual_snapshot("band", &[]);
    let NodeBody::Device(device) = &mut snapshot.graph.nodes[0].body else {
        panic!("the starter opens with a receiver");
    };
    device.locked_streams = vec![0];
    let workspace = put_active_workspace(&app, &snapshot).await;
    apply(&app, workspace).await;

    let (status, _) = act(&app, "device", tune(101e6)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = act(&app, "scope", tune(101e6)).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a scope tunes the lane it watches"
    );

    let NodeBody::Device(device) = &mut snapshot.graph.nodes[0].body else {
        panic!("the starter opens with a receiver");
    };
    device.locked_streams.clear();
    put_active_workspace(&app, &snapshot).await;
    assert_eq!(acted(&app, "scope", tune(101e6)).await, NodeActed::Tuned);
    let settings = &get_state(&app).await.device_sets[0].settings;
    assert_eq!(settings.center_hz, Some(101e6));
    assert_eq!(settings.tuning, Some(sdrmm_wire::Tuning::Manual));
}

#[tokio::test]
async fn retyping_drops_a_wire_the_new_decoder_refuses_with_a_notice() {
    let app = test_router();
    let mut snapshot = virtual_snapshot("band", &[("voice", "nfm", "iq")]);
    snapshot.graph.nodes.push(sdrmm_wire::PatchNode {
        id: "loud".to_owned(),
        body: NodeBody::Speaker,
        position: sdrmm_wire::Position { x: 800.0, y: 300.0 },
        size: None,
        label: None,
    });
    snapshot.graph.edges.push(sdrmm_wire::PatchEdge {
        from: sdrmm_wire::PortRef {
            node: "voice".to_owned(),
            port: "audio".to_owned(),
        },
        to: sdrmm_wire::PortRef {
            node: "loud".to_owned(),
            port: "audio".to_owned(),
        },
    });
    let workspace = put_active_workspace(&app, &snapshot).await;
    apply(&app, workspace).await;
    let frequency = live_frequency(&get_state(&app).await, "nfm");

    let retype = serde_json::json!({"action": "retype", "with": {"channel_type": "pocsag"}});
    let NodeActed::Retyped { dropped } = acted(&app, "voice", retype).await else {
        panic!("a retype reports what it dropped");
    };
    assert_eq!(dropped.len(), 1, "{dropped:?}");
    assert_eq!(dropped[0].to.node, "loud");

    let detail = workspace_detail(&app, workspace).await;
    let Some(NodeBody::Channel(channel)) = detail.snapshot.graph.node("voice").map(|n| &n.body)
    else {
        panic!("still a channel");
    };
    assert_eq!(channel.channel_type, "pocsag");
    assert!(
        !detail
            .snapshot
            .graph
            .edges
            .iter()
            .any(|edge| edge.to.node == "loud")
    );
    assert!(detail.notices.iter().any(|notice| matches!(
        &notice.notice,
        WorkspaceNoticeKind::DroppedWires { wires } if wires == &dropped
    )));
    assert!((live_frequency(&get_state(&app).await, "pocsag") - frequency).abs() < 1.0);
}

#[tokio::test]
async fn removing_a_device_node_closes_its_radio() {
    let app = test_router();
    let snapshot = virtual_snapshot("band", &[("voice", "nfm", "iq")]);
    let workspace = put_active_workspace(&app, &snapshot).await;
    apply(&app, workspace).await;
    assert_eq!(get_state(&app).await.device_sets.len(), 1);

    let mut without = snapshot.clone();
    without.graph.nodes.retain(|node| node.id != "device");
    without
        .graph
        .edges
        .retain(|edge| edge.from.node != "device" && edge.to.node != "device");
    put_active_workspace(&app, &without).await;
    assert!(get_state(&app).await.device_sets.is_empty());
}

#[tokio::test]
async fn removing_a_channel_node_closes_its_decoder() {
    let app = test_router();
    let snapshot = virtual_snapshot("band", &[("voice", "nfm", "iq"), ("other", "am", "iq")]);
    let workspace = put_active_workspace(&app, &snapshot).await;
    apply(&app, workspace).await;

    let mut without = snapshot.clone();
    without.graph.nodes.retain(|node| node.id != "voice");
    without.graph.edges.retain(|edge| edge.to.node != "voice");
    put_active_workspace(&app, &without).await;
    let channels = &get_state(&app).await.device_sets[0].channels;
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].node.as_deref(), Some("other"));
}

#[tokio::test]
async fn saving_pads_explicit_array_geometry_to_its_wired_lanes() {
    let app = test_router();
    let mut snapshot = sdrmm_wire::WorkspaceSnapshot::empty();
    for id in ["a", "b"] {
        snapshot.graph.nodes.push(sdrmm_wire::PatchNode {
            id: id.to_owned(),
            body: NodeBody::Device(sdrmm_wire::DeviceNode::default()),
            position: sdrmm_wire::Position { x: 0.0, y: 0.0 },
            size: None,
            label: None,
        });
    }
    snapshot.graph.nodes.push(sdrmm_wire::PatchNode {
        id: "arr".to_owned(),
        body: NodeBody::Array(sdrmm_wire::ArrayNode {
            geometry: sdrmm_wire::ArrayGeometry::Explicit {
                positions: vec![sdrmm_wire::ArrayElement::default()],
            },
            ..sdrmm_wire::ArrayNode::default()
        }),
        position: sdrmm_wire::Position { x: 400.0, y: 0.0 },
        size: None,
        label: None,
    });
    for (from, lane) in [("a", "lane"), ("b", "lane2")] {
        snapshot.graph.edges.push(sdrmm_wire::PatchEdge {
            from: sdrmm_wire::PortRef {
                node: from.to_owned(),
                port: "iq".to_owned(),
            },
            to: sdrmm_wire::PortRef {
                node: "arr".to_owned(),
                port: lane.to_owned(),
            },
        });
    }
    let workspace = put_active_workspace(&app, &snapshot).await;
    let detail = workspace_detail(&app, workspace).await;
    let Some(NodeBody::Array(array)) = detail.snapshot.graph.node("arr").map(|n| &n.body) else {
        panic!("the array is stored");
    };
    let sdrmm_wire::ArrayGeometry::Explicit { positions } = &array.geometry else {
        panic!("still explicit");
    };
    assert_eq!(positions.len(), 2);
}

struct Answering;

impl DeviceDriver for Answering {
    fn id(&self) -> &'static str {
        "answering"
    }

    fn probe(&self) -> Vec<DeviceInfo> {
        Vec::new()
    }

    fn open(&self, _info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        Err(DeviceError::Io("not in this test".to_owned()))
    }

    fn resolve(&self, key: &str) -> Option<DeviceInfo> {
        Some(DeviceInfo {
            driver: "answering".to_owned(),
            key: key.to_owned(),
            label: format!("answering {key}"),
            serial: None,
            profile: None,
        })
    }
}

#[tokio::test]
async fn a_network_radio_is_adopted_without_opening() {
    let mut registry = sdrmm_device::DeviceRegistry::new();
    registry.register(1, Box::new(Answering));
    let state = AppState::new(
        Engine::with_registry(registry, None),
        Arc::new(Store::open(None).expect("in-memory store")),
    );
    let (app, background) = router_with_state(state, &ServerOptions::default());
    background.detach();

    let (status, body) = request(
        app.clone(),
        "POST",
        "/api/devices/network",
        Some(r#"{"address":"answering:10.0.0.2:1234"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let device: DeviceInfo = serde_json::from_slice(&body).expect("json");
    assert_eq!(device.key, "10.0.0.2:1234");
    assert!(get_state(&app).await.device_sets.is_empty());

    let (status, _) = request(
        app.clone(),
        "POST",
        "/api/devices/network",
        Some(r#"{"address":"nobody:10.0.0.3"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

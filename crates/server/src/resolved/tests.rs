use std::sync::Arc;

use axum::http::StatusCode;
use sdrmm_wire::{
    ChannelNode, ChannelSettings, DeviceRef, NodeBody, PatchEdge, PatchNode, PortRef, Position,
    ResolvedState, Runs, UpdateWorkspaceRequest, WorkspaceSnapshot,
};
use tokio::sync::broadcast;

use super::*;

struct Bench {
    engine: Arc<Engine>,
    store: Store,
    board: ProblemBoard,
    hub: ResolvedHub,
    workspace: i64,
}

impl Bench {
    fn new() -> Self {
        let mut registry = sdrmm_device::DeviceRegistry::new();
        registry.register(1, Box::new(sdrmm_device_virtual::VirtualDriver::new()));
        let engine = Engine::with_registry(registry, None);
        let store = Store::open(None).expect("store");
        let workspace = store
            .create_workspace("bench", &snapshot(&["voice"]))
            .expect("create");
        store.activate_workspace(workspace).expect("activate");
        Self {
            engine,
            store,
            board: ProblemBoard::default(),
            hub: ResolvedHub::default(),
            workspace,
        }
    }

    fn reload(&self) -> bool {
        self.hub.reload(&self.store, &self.engine, &self.board)
    }

    fn rebind(&self) -> bool {
        self.hub.rebind(&self.engine, &self.board)
    }

    fn save(&self, channels: &[&str]) {
        let revision = self
            .store
            .workspace(self.workspace)
            .expect("workspace")
            .info
            .revision;
        self.store
            .update_workspace(
                self.workspace,
                &UpdateWorkspaceRequest {
                    revision,
                    name: None,
                    snapshot: Some(snapshot(channels)),
                },
                None,
            )
            .expect("update");
    }

    fn epoch(&self) -> u64 {
        self.hub.current().patch.epoch
    }
}

fn snapshot(channels: &[&str]) -> WorkspaceSnapshot {
    let mut snapshot = WorkspaceSnapshot::starter();
    if let NodeBody::Device(device) = &mut snapshot.graph.nodes[0].body {
        device.device = Some(DeviceRef {
            backend: "virtual".to_owned(),
            serial: None,
            key: Some("band".to_owned()),
        });
    }
    for id in channels {
        snapshot.graph.nodes.push(PatchNode {
            id: (*id).to_owned(),
            body: NodeBody::Channel(ChannelNode {
                channel_type: "nfm".to_owned(),
                tuning_locked: false,
            }),
            position: Position { x: 0.0, y: 0.0 },
            size: None,
            label: None,
        });
        snapshot.graph.edges.push(PatchEdge {
            from: PortRef {
                node: "device".to_owned(),
                port: "iq".to_owned(),
            },
            to: PortRef {
                node: (*id).to_owned(),
                port: "iq".to_owned(),
            },
        });
    }
    snapshot
}

fn published(rx: &mut broadcast::Receiver<ServerEvent>) -> usize {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter(|event| matches!(event, ServerEvent::PatchResolved(_)))
        .count()
}

#[test]
fn a_graph_edit_publishes_once() {
    let bench = Bench::new();
    let mut rx = bench.engine.subscribe_events();
    assert!(bench.reload());
    assert!(!bench.reload(), "nothing changed, nothing to publish");
    assert_eq!(published(&mut rx), 1);
    let before = bench.epoch();

    bench.save(&["voice", "other"]);
    assert!(bench.reload());
    assert!(!bench.reload());
    assert_eq!(published(&mut rx), 1);
    assert_eq!(bench.epoch(), before + 1);
    assert!(bench.hub.current().patch.node("other").is_some());
}

#[test]
fn opening_a_radio_publishes_once_with_a_rising_epoch() {
    let bench = Bench::new();
    bench.reload();
    let before = bench.epoch();
    assert!(matches!(
        bench.hub.current().patch.runs("device"),
        Some(Runs::Unbound { .. })
    ));
    let mut rx = bench.engine.subscribe_events();

    let set = bench
        .engine
        .create_device_set("virtual:band")
        .expect("radio");
    assert!(bench.rebind());
    assert!(!bench.rebind());
    assert_eq!(published(&mut rx), 1);
    assert_eq!(bench.epoch(), before + 1);
    assert_eq!(bench.hub.current().patch.device_set("device"), Some(set));
}

#[test]
fn a_scanner_step_publishes_nothing() {
    let bench = Bench::new();
    let set = bench
        .engine
        .create_device_set("virtual:band")
        .expect("radio");
    let settings = ChannelSettings::default_for("nfm").expect("nfm");
    let channel = bench
        .engine
        .add_channel_for(set, 0, settings.clone(), Some("voice"))
        .expect("channel");
    bench.reload();
    assert!(bench.hub.current().patch.channel("voice").is_some());
    let epoch = bench.epoch();
    let mut rx = bench.engine.subscribe_events();

    bench
        .engine
        .patch_channel(
            set,
            channel,
            ChannelSettings {
                frequency_hz: settings.frequency_hz + 25_000.0,
                ..settings
            },
        )
        .expect("retune");
    assert!(!bench.rebind());
    assert_eq!(published(&mut rx), 0);
    assert_eq!(bench.epoch(), epoch);
}

#[test]
fn a_missing_radio_is_a_problem_on_its_node() {
    let bench = Bench::new();
    bench.reload();
    assert_eq!(bench.board.of("device"), ["Radio not open"]);
    bench
        .engine
        .create_device_set("virtual:band")
        .expect("radio");
    bench.rebind();
    assert!(bench.board.of("device").is_empty());
}

#[tokio::test]
async fn the_active_workspace_is_served_resolved() {
    let (app, state) = crate::tests::test_router_with_state();
    state
        .engine
        .create_device_set("virtual:band")
        .expect("the radio opens");
    let workspace = state
        .store
        .active_workspace_id()
        .expect("store")
        .expect("seeded workspace");
    let revision = state
        .store
        .workspace(workspace)
        .expect("workspace")
        .info
        .revision;
    state
        .store
        .update_workspace(
            workspace,
            &UpdateWorkspaceRequest {
                revision,
                name: None,
                snapshot: Some(snapshot(&["voice"])),
            },
            None,
        )
        .expect("update");
    state
        .resolved
        .reload(&state.store, &state.engine, &state.problems);

    let (status, body) =
        crate::tests::request(app, "GET", "/api/workspaces/active/resolved", None).await;
    assert_eq!(status, StatusCode::OK);
    let served: ResolvedState = serde_json::from_slice(&body).expect("json");
    assert_eq!(served.patch.workspace, Some(workspace));
    let voice = served.patch.node("voice").expect("voice resolved");
    assert_eq!(voice.input("iq").map(|port| port.feeds.len()), Some(1));
    assert!(served.patch.device_set("device").is_some());
    let raw: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(raw["patch"]["nodes"][0]["runs"]["runs"], "radio");
}

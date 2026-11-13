use std::sync::atomic::{AtomicBool, Ordering};

use sdrmm_device::{DeviceDriver, DeviceError, DeviceRegistry, SdrDevice};
use sdrmm_device_virtual::VirtualDriver;
use sdrmm_wire::{DeviceBackend, DeviceInfo, DeviceRef, NodeBody, StateScope};

use super::*;

const SERIAL: &str = "late-1";

struct LateDriver {
    present: Arc<AtomicBool>,
    radios: VirtualDriver,
}

impl DeviceDriver for LateDriver {
    fn id(&self) -> DeviceBackend {
        DeviceBackend::Spyserver
    }

    fn probe(&self) -> Vec<DeviceInfo> {
        if !self.present.load(Ordering::SeqCst) {
            return Vec::new();
        }
        vec![DeviceInfo {
            driver: DeviceBackend::Spyserver,
            key: "band".to_string(),
            label: "Late radio".to_string(),
            serial: Some(SERIAL.to_string()),
            profile: None,
        }]
    }

    fn open(&self, info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        self.radios.open(&DeviceInfo {
            driver: DeviceBackend::Virtual,
            ..info.clone()
        })
    }
}

fn late_state(present: &Arc<AtomicBool>, store: &Arc<Store>) -> AppState {
    let mut registry = DeviceRegistry::new();
    registry.register(
        1,
        Box::new(LateDriver {
            present: present.clone(),
            radios: VirtualDriver::new(),
        }),
    );
    AppState::new(Engine::with_registry(registry, None), store.clone())
}

fn late_snapshot() -> sdrmm_wire::WorkspaceSnapshot {
    let mut snapshot = virtual_snapshot("band", &[]);
    let NodeBody::Device(node) = &mut snapshot.graph.nodes[0].body else {
        panic!("the default workspace opens with a receiver")
    };
    node.device = Some(DeviceRef {
        backend: DeviceBackend::Spyserver,
        serial: Some(SERIAL.to_string()),
        key: None,
    });
    snapshot
}

async fn bound_eventually(state: &AppState, why: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while state.engine.snapshot().device_sets.is_empty() {
        assert!(Instant::now() < deadline, "{why}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_restarted_server_opens_the_active_workspaces_radio_without_a_client() {
    let store = Arc::new(Store::open(None).expect("in-memory store"));
    let absent = Arc::new(AtomicBool::new(false));
    let (first, background) =
        router_with_state(late_state(&absent, &store), &ServerOptions::default());
    background.detach();
    put_active_workspace(&first, &late_snapshot()).await;

    let present = Arc::new(AtomicBool::new(true));
    let restarted = late_state(&present, &store);
    let (_app, background) = router_with_state(restarted.clone(), &ServerOptions::default());
    background.detach();
    bound_eventually(
        &restarted,
        "the restarted server never opened the saved radio",
    )
    .await;
}

#[tokio::test]
async fn a_radio_that_turns_up_later_is_bound_to_the_node_waiting_for_it() {
    let present = Arc::new(AtomicBool::new(false));
    let mut registry = DeviceRegistry::new();
    registry.register(
        1,
        Box::new(LateDriver {
            present: present.clone(),
            radios: VirtualDriver::new(),
        }),
    );
    let state = AppState::new(
        Engine::with_registry(registry, None),
        Arc::new(Store::open(None).expect("in-memory store")),
    );
    let (app, background) = router_with_state(state.clone(), &ServerOptions::default());
    background.detach();
    let mut snapshot = virtual_snapshot("band", &[]);
    let NodeBody::Device(node) = &mut snapshot.graph.nodes[0].body else {
        panic!("the default workspace opens with a receiver")
    };
    node.device = Some(DeviceRef {
        backend: DeviceBackend::Spyserver,
        serial: Some(SERIAL.to_string()),
        key: None,
    });
    put_active_workspace(&app, &snapshot).await;
    assert!(state.engine.snapshot().device_sets.is_empty());

    present.store(true, Ordering::SeqCst);
    state.engine.emit_scope(StateScope::Devices);

    let deadline = Instant::now() + Duration::from_secs(5);
    while state.engine.snapshot().device_sets.is_empty() {
        assert!(
            Instant::now() < deadline,
            "the radio turned up but was never bound"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        state.engine.snapshot().device_sets[0]
            .device
            .serial
            .as_deref(),
        Some(SERIAL)
    );
}

#[tokio::test]
async fn the_startup_pass_leaves_a_workspace_a_client_edited_first_alone() {
    let store = Arc::new(Store::open(None).expect("in-memory store"));
    let absent = Arc::new(AtomicBool::new(false));
    let (first, background) =
        router_with_state(late_state(&absent, &store), &ServerOptions::default());
    background.detach();
    let workspace = put_active_workspace(&first, &late_snapshot()).await;

    let present = Arc::new(AtomicBool::new(true));
    let restarted = late_state(&present, &store);
    let (_app, background) = router_with_state(restarted.clone(), &ServerOptions::default());
    background.detach();
    let revision = store.workspace(workspace).expect("stored").info.revision;
    let mut edited = late_snapshot();
    edited.graph.nodes[0].position.x += 10.0;
    store
        .update_workspace(
            workspace,
            &sdrmm_wire::UpdateWorkspaceRequest {
                revision,
                name: None,
                snapshot: Some(edited),
            },
            None,
        )
        .expect("the client's edit lands");

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        restarted.engine.snapshot().device_sets.is_empty(),
        "the startup pass opened a workspace the client had taken over"
    );
}

fn quiet_workspace(store: &Store) -> i64 {
    store
        .create_workspace("Quiet", &sdrmm_wire::WorkspaceSnapshot::starter())
        .expect("second workspace")
}

/// Holds the apply gate while `work` starts, switches to `quiet` behind it, then lets it run.
fn switch_while_gated(state: &AppState, quiet: i64, work: impl FnOnce()) {
    let gate = state.apply_gate.clone();
    let held = rest::lock_gate(&gate);
    work();
    std::thread::sleep(Duration::from_millis(200));
    state.store.activate_workspace(quiet).expect("switched");
    drop(held);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_radio_that_turns_up_during_a_switch_binds_the_new_workspace() {
    let store = Arc::new(Store::open(None).expect("in-memory store"));
    let present = Arc::new(AtomicBool::new(false));
    let state = late_state(&present, &store);
    let (app, background) = router_with_state(state.clone(), &ServerOptions::default());
    background.detach();
    put_active_workspace(&app, &late_snapshot()).await;
    let quiet = quiet_workspace(&store);
    present.store(true, Ordering::SeqCst);

    let mut binding = None;
    switch_while_gated(&state, quiet, || {
        binding = Some(tokio::spawn(crate::rebind::bind_new_radios(state.clone())));
    });
    binding.expect("spawned").await.expect("bound");

    assert!(
        state.engine.snapshot().device_sets.is_empty(),
        "the old workspace's radio was opened after the switch"
    );
}

#[test]
fn a_run_controller_settles_the_workspace_active_once_it_holds_the_gate() {
    use sdrmm_wire::{CONTROL_PORT, DeviceNode, PatchEdge, PortRef, Position, ScheduleNode};
    let store = Arc::new(Store::open(None).expect("in-memory store"));
    let node = |id: &str, body: NodeBody| sdrmm_wire::PatchNode {
        id: id.to_owned(),
        body,
        position: Position { x: 0.0, y: 0.0 },
        size: None,
        label: None,
    };
    let port = |node: &str| PortRef {
        node: node.to_owned(),
        port: CONTROL_PORT.to_owned(),
    };
    let mut gated = sdrmm_wire::WorkspaceSnapshot::starter();
    gated.graph.nodes = vec![
        node("day", NodeBody::Schedule(ScheduleNode::default())),
        node(
            "dev",
            NodeBody::Device(DeviceNode {
                device: Some(DeviceRef {
                    backend: DeviceBackend::Virtual,
                    serial: None,
                    key: Some("band".to_owned()),
                }),
                ..DeviceNode::default()
            }),
        ),
    ];
    gated.graph.edges = vec![PatchEdge {
        from: port("day"),
        to: port("dev"),
    }];
    let wired = store.create_workspace("Wired", &gated).expect("wired");
    store.activate_workspace(wired).expect("active");
    let quiet = quiet_workspace(&store);
    let state = AppState::new(Engine::new(None), store);

    let mut settling = None;
    switch_while_gated(&state, quiet, || {
        let state = state.clone();
        settling = Some(std::thread::spawn(move || {
            crate::run_gate::settle(&state, std::collections::BTreeSet::new(), false)
        }));
    });
    let resting = settling.expect("spawned").join().expect("settled");

    assert!(
        resting.is_empty(),
        "settled the workspace switched away from: {resting:?}"
    );
}

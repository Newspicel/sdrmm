use std::sync::atomic::{AtomicBool, Ordering};

use sdrmm_device::{DeviceDriver, DeviceError, DeviceRegistry, SdrDevice};
use sdrmm_device_virtual::VirtualDriver;
use sdrmm_wire::{DeviceInfo, DeviceRef, NodeBody, StateScope};

use super::*;

const SERIAL: &str = "late-1";

struct LateDriver {
    present: Arc<AtomicBool>,
    radios: VirtualDriver,
}

impl DeviceDriver for LateDriver {
    fn id(&self) -> &'static str {
        "late"
    }

    fn probe(&self) -> Vec<DeviceInfo> {
        if !self.present.load(Ordering::SeqCst) {
            return Vec::new();
        }
        vec![DeviceInfo {
            driver: "late".to_string(),
            key: "band".to_string(),
            label: "Late radio".to_string(),
            serial: Some(SERIAL.to_string()),
            profile: None,
        }]
    }

    fn open(&self, info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        self.radios.open(&DeviceInfo {
            driver: "virtual".to_string(),
            ..info.clone()
        })
    }
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
        backend: "late".to_string(),
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

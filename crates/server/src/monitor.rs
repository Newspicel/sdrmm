use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

use sdrmm_engine::{Engine, monitor::MonitorHandle};
use sdrmm_wire::{
    DecodedRecord, DecoderEvent, DeviceSetStatus, EVENTS_PORT, EventOrigin, IQ_PORT, Keeps,
    NodeBody, ProblemSource, ServerEvent, SpectrumMonitorNode, StateScope,
};
use tokio::sync::{broadcast::error::RecvError, watch};

use crate::{calls::Calls, problems::ProblemBoard, resolved::Resolved};

#[derive(Clone, PartialEq)]
struct Binding {
    node: String,
    device_set: u32,
    stream: u32,
    settings: SpectrumMonitorNode,
}

struct Active {
    binding: Binding,
    _handle: MonitorHandle,
}

pub(crate) struct Watch {
    pub(crate) engine: Weak<Engine>,
    pub(crate) resolved: watch::Receiver<Arc<Resolved>>,
    pub(crate) problems: Arc<ProblemBoard>,
    pub(crate) calls: Arc<Calls>,
}

pub(crate) async fn run(mut watch: Watch) {
    let Some(strong) = watch.engine.upgrade() else {
        return;
    };
    let mut events = strong.subscribe_events();
    drop(strong);
    let mut active = HashMap::<String, Active>::new();
    loop {
        let Some(engine) = watch.engine.upgrade() else {
            return;
        };
        let resolved = watch.resolved.borrow_and_update().clone();
        let (problems, calls) = (watch.problems.clone(), watch.calls.clone());
        let reconciled = tokio::task::spawn_blocking(move || {
            reconcile(&engine, &resolved, &calls, &problems, &mut active);
            active
        })
        .await;
        match reconciled {
            Ok(next) => active = next,
            Err(error) => {
                tracing::error!(%error, "spectrum monitor reconciliation failed");
                return;
            }
        }
        if !next_change(&mut watch.resolved, &mut events).await {
            return;
        }
    }
}

async fn next_change(
    resolved: &mut watch::Receiver<Arc<Resolved>>,
    events: &mut tokio::sync::broadcast::Receiver<ServerEvent>,
) -> bool {
    loop {
        tokio::select! {
            changed = resolved.changed() => return changed.is_ok(),
            event = events.recv() => match event {
                Ok(ServerEvent::StateChanged {
                    scope: StateScope::All | StateScope::Devices | StateScope::DeviceSet(_),
                })
                | Err(RecvError::Lagged(_)) => return true,
                Ok(_) => {}
                Err(RecvError::Closed) => return false,
            },
        }
    }
}

fn desired(engine: &Engine, resolved: &Resolved) -> Vec<Binding> {
    let snapshot = engine.snapshot();
    resolved
        .graph
        .nodes
        .iter()
        .filter_map(|node| {
            let NodeBody::SpectrumMonitor(settings) = &node.body else {
                return None;
            };
            let (device_set, stream) = resolved
                .patch
                .first_feed(&node.id, IQ_PORT)?
                .tap
                .as_ref()?
                .lane()?;
            snapshot
                .device_sets
                .iter()
                .any(|set| set.id == device_set && set.status == DeviceSetStatus::Running)
                .then(|| Binding {
                    node: node.id.clone(),
                    device_set,
                    stream,
                    settings: SpectrumMonitorNode {
                        keep_clips: resolved.patch.keeps(&node.id, EVENTS_PORT, Keeps::Clips),
                        ..settings.clone()
                    },
                })
        })
        .collect()
}

fn reconcile(
    engine: &Arc<Engine>,
    resolved: &Resolved,
    calls: &Arc<Calls>,
    problems: &ProblemBoard,
    active: &mut HashMap<String, Active>,
) {
    let desired = desired(engine, resolved);
    active.retain(|_, running| running._handle.is_active() && desired.contains(&running.binding));
    for node in resolved.graph.nodes.iter().filter(|node| {
        matches!(node.body, NodeBody::SpectrumMonitor(_))
            && !desired.iter().any(|binding| binding.node == node.id)
    }) {
        problems.set(engine, ProblemSource::Monitor, &node.id, Vec::new());
    }
    for binding in desired {
        if active.contains_key(&binding.node) {
            continue;
        }
        let node = binding.node.clone();
        match start(engine, calls, &binding) {
            Ok(handle) => {
                problems.set(engine, ProblemSource::Monitor, &node, Vec::new());
                active.insert(
                    node,
                    Active {
                        binding,
                        _handle: handle,
                    },
                );
            }
            Err(error) => {
                tracing::warn!(%error, node, "spectrum monitor could not start");
                problems.set(
                    engine,
                    ProblemSource::Monitor,
                    &node,
                    vec![error.to_string()],
                );
            }
        }
    }
}

fn start(
    engine: &Arc<Engine>,
    calls: &Arc<Calls>,
    binding: &Binding,
) -> Result<MonitorHandle, sdrmm_engine::EngineError> {
    let weak = Arc::downgrade(engine);
    let calls = calls.clone();
    let origin = binding.node.clone();
    let device_set = binding.device_set;
    engine.monitor(
        device_set,
        binding.stream,
        binding.settings.clone(),
        move |mut output| {
            let Some(engine) = weak.upgrade() else {
                return;
            };
            if !output.audio.is_empty()
                && let DecoderEvent::Transmission(transmission) = &mut output.event
            {
                let (audio, evicted) = calls.store_clip(&output.audio);
                transmission.audio = Some(audio);
                if evicted {
                    transmission.error.get_or_insert_with(|| {
                        "older audio evicted by the temporary buffer limit".to_owned()
                    });
                }
            }
            let at = match &output.event {
                DecoderEvent::Transmission(t) => t.ended_at.clone(),
                _ => None,
            }
            .unwrap_or_else(|| format!("{:.9}", jiff::Timestamp::now()));
            engine.publish_decoded(DecodedRecord {
                origin: Some(EventOrigin {
                    node: origin.clone(),
                    transmission: output.transmission,
                }),
                device_set,
                channel: u32::MAX,
                at,
                freq_hz: output.frequency_hz,
                event: output.event,
                sinks: Vec::new(),
                callsign: None,
                view: None,
            });
        },
    )
}

#[cfg(test)]
mod tests {
    use sdrmm_device::DeviceRegistry;
    use sdrmm_wire::{DeviceRef, PatchEdge, PatchNode, PortRef, Position, WorkspaceSnapshot};

    use super::*;
    use crate::{Store, resolved::ResolvedHub};

    struct Bench {
        engine: Arc<Engine>,
        store: Store,
        hub: ResolvedHub,
        problems: ProblemBoard,
        calls: Arc<Calls>,
        set: u32,
    }

    impl Bench {
        fn new() -> Self {
            let mut registry = DeviceRegistry::new();
            registry.register(1, Box::new(sdrmm_device_virtual::VirtualDriver::new()));
            let engine = Engine::with_registry(registry, None);
            let set = engine
                .create_device_set("virtual:band")
                .expect("virtual radio");
            Self {
                engine,
                store: Store::open(None).expect("store"),
                hub: ResolvedHub::default(),
                problems: ProblemBoard::default(),
                calls: Arc::new(Calls::default()),
                set,
            }
        }

        fn activate(&self, snapshot: &WorkspaceSnapshot) {
            let name = format!(
                "monitor {}",
                self.store.list_workspaces().expect("list").workspaces.len()
            );
            let id = self
                .store
                .create_workspace(&name, snapshot)
                .expect("workspace");
            self.store.activate_workspace(id).expect("activate");
            self.hub.reload(&self.store, &self.engine, &self.problems);
        }

        fn reconcile(&self, active: &mut HashMap<String, Active>) {
            reconcile(
                &self.engine,
                &self.hub.current(),
                &self.calls,
                &self.problems,
                active,
            );
        }
    }

    fn watching(port: &str) -> WorkspaceSnapshot {
        let mut snapshot = WorkspaceSnapshot::starter();
        if let Some(NodeBody::Device(device)) = snapshot
            .graph
            .nodes
            .iter_mut()
            .find(|node| node.id == "device")
            .map(|node| &mut node.body)
        {
            device.device = Some(DeviceRef {
                backend: "virtual".to_owned(),
                serial: None,
                key: Some("band".to_owned()),
            });
        }
        snapshot.graph.nodes.push(PatchNode {
            id: "monitor".to_owned(),
            body: NodeBody::SpectrumMonitor(SpectrumMonitorNode::default()),
            position: Position { x: 100.0, y: 0.0 },
            size: None,
            label: None,
        });
        snapshot.graph.edges.push(PatchEdge {
            from: PortRef {
                node: "device".to_owned(),
                port: port.to_owned(),
            },
            to: PortRef {
                node: "monitor".to_owned(),
                port: "iq".to_owned(),
            },
        });
        snapshot
    }

    #[test]
    fn wiring_creates_one_monitor_and_unwiring_releases_it_without_channels() {
        let bench = Bench::new();
        bench.activate(&watching("iq"));
        let mut active = HashMap::new();
        bench.reconcile(&mut active);
        assert_eq!(active.len(), 1);
        let id = active["monitor"].binding.clone();
        bench.reconcile(&mut active);
        assert_eq!(active.len(), 1);
        assert!(active["monitor"].binding == id);
        assert!(
            bench
                .engine
                .snapshot()
                .device_sets
                .iter()
                .find(|device| device.id == bench.set)
                .is_some_and(|device| device.channels.is_empty())
        );
        bench.activate(&WorkspaceSnapshot::empty());
        bench.reconcile(&mut active);
        assert!(active.is_empty());
    }

    #[test]
    fn the_monitor_keeps_clips_only_while_its_events_reach_a_log() {
        let bench = Bench::new();
        bench.activate(&watching("iq"));
        let mut active = HashMap::new();
        bench.reconcile(&mut active);
        assert!(!active["monitor"].binding.settings.keep_clips);

        let mut wired = watching("iq");
        wired.graph.nodes.push(PatchNode {
            id: "log".to_owned(),
            body: NodeBody::default_for(sdrmm_wire::NodeKind::DecoderLog),
            position: Position { x: 200.0, y: 0.0 },
            size: None,
            label: None,
        });
        wired.graph.edges.push(PatchEdge {
            from: PortRef {
                node: "monitor".to_owned(),
                port: EVENTS_PORT.to_owned(),
            },
            to: PortRef {
                node: "log".to_owned(),
                port: EVENTS_PORT.to_owned(),
            },
        });
        bench.activate(&wired);
        bench.reconcile(&mut active);
        assert!(active["monitor"].binding.settings.keep_clips);
    }

    #[test]
    fn a_monitor_that_cannot_start_is_a_problem_not_a_record() {
        let bench = Bench::new();
        let mut decoded = bench.engine.subscribe_decoded();
        bench.activate(&watching("iq4"));
        let mut active = HashMap::new();
        bench.reconcile(&mut active);
        assert!(active.is_empty());
        assert_eq!(bench.problems.of("monitor").len(), 1);
        assert!(decoded.try_recv().is_err(), "a failure became a record");

        bench.activate(&watching("iq"));
        bench.reconcile(&mut active);
        assert_eq!(active.len(), 1);
        assert!(bench.problems.of("monitor").is_empty());
    }
}

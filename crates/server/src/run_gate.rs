use std::{
    collections::BTreeSet,
    sync::{Mutex, MutexGuard, PoisonError},
};

use sdrmm_wire::PatchGraph;
use tokio::sync::watch;

use crate::{AppState, apply, rest::lock_gate, workspace};

/// Which controllers hold which nodes on. A radio or recorder with a wired run input rests until
/// one of its controllers says on. The saved patch never changes: [`RunGate::gated`] lays the
/// holds over it wherever the server reads the patch.
#[derive(Default)]
pub(crate) struct RunGate {
    held: Mutex<BTreeSet<(String, String)>>,
    changes: watch::Sender<u64>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl RunGate {
    pub(crate) fn hold(&self, from: &str, target: &str, on: bool) {
        let key = (target.to_owned(), from.to_owned());
        let changed = {
            let mut held = locked(&self.held);
            if on {
                held.insert(key)
            } else {
                held.remove(&key)
            }
        };
        if changed {
            self.changes.send_modify(|count| *count += 1);
        }
    }

    pub(crate) fn retain(&self, live: impl Fn(&str) -> bool) {
        locked(&self.held).retain(|(target, from)| live(target) && live(from));
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    pub(crate) fn gated(&self, graph: &PatchGraph) -> PatchGraph {
        let held = locked(&self.held).clone();
        graph.rested(|target, from| held.contains(&(target.to_owned(), from.to_owned())))
    }
}

fn resting_radios(graph: &PatchGraph) -> BTreeSet<String> {
    graph
        .device_nodes()
        .filter(|node| node.body.waiting())
        .map(|node| node.id.clone())
        .collect()
}

/// Closes a radio when its controllers let it rest and opens it again when one says on.
pub(crate) fn follow(state: &AppState) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        tracing::warn!("no runtime in context: wired radios will not follow their controllers");
        return;
    };
    let mut holds = state.runs.subscribe();
    let mut patch = state.resolved.subscribe();
    let state = state.clone();
    handle.spawn(async move {
        let mut resting = BTreeSet::new();
        loop {
            let moved = tokio::select! {
                changed = holds.changed() => changed.map(|()| true),
                changed = patch.changed() => changed.map(|()| false),
            };
            let Ok(moved) = moved else {
                return;
            };
            let app = state.clone();
            let was = std::mem::take(&mut resting);
            match tokio::task::spawn_blocking(move || settle(&app, was, moved)).await {
                Ok(now) => resting = now,
                Err(error) => {
                    tracing::error!(%error, "following run controllers stopped");
                    return;
                }
            }
        }
    });
}

fn settle(state: &AppState, was: BTreeSet<String>, moved: bool) -> BTreeSet<String> {
    let active = match state.store.active_workspace() {
        Ok(Some(active)) => active,
        Ok(None) => return BTreeSet::new(),
        Err(error) => {
            tracing::error!(%error, "could not read the active workspace to run its radios");
            return was;
        }
    };
    let saved = &active.snapshot.graph;
    let now = resting_radios(&state.runs.gated(saved));
    let rested: Vec<&String> = now.difference(&was).collect();
    let woke = was.difference(&now).next().is_some();
    if !rested.is_empty() || woke {
        let _serialized = lock_gate(&state.apply_gate);
        close(state, saved, &rested);
        if woke && let Err(error) = apply::bring_up_active(state, active.info.id) {
            tracing::warn!(?error, "could not open a radio its controller woke");
        }
    }
    if moved || !rested.is_empty() || woke {
        state.resolve_now();
    }
    now
}

fn close(state: &AppState, saved: &PatchGraph, rested: &[&String]) {
    let bound: Vec<(String, u32)> = workspace::bind_devices(saved, &state.engine.snapshot())
        .into_iter()
        .filter(|(node, _)| rested.contains(&node))
        .collect();
    if bound.is_empty() {
        return;
    }
    if let Err(error) = workspace::save_active(state) {
        tracing::warn!(%error, "could not save a radio's settings before it rests");
    }
    for (node, device_set) in bound {
        match state.engine.remove_device_set(device_set) {
            Ok(()) => tracing::info!(node, device_set, "a radio rests until its controller"),
            Err(error) => tracing::warn!(node, device_set, %error, "could not rest a radio"),
        }
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{
        AudioRecorderNode, CONTROL_PORT, DeviceNode, NodeBody, PatchEdge, PatchNode, PortRef,
        Position, ScheduleNode,
    };

    use super::*;

    fn node(id: &str, body: NodeBody) -> PatchNode {
        PatchNode {
            id: id.to_owned(),
            body,
            position: Position { x: 0.0, y: 0.0 },
            size: None,
            label: None,
        }
    }

    fn run_wire(from: &str, to: &str) -> PatchEdge {
        PatchEdge {
            from: PortRef {
                node: from.to_owned(),
                port: CONTROL_PORT.to_owned(),
            },
            to: PortRef {
                node: to.to_owned(),
                port: CONTROL_PORT.to_owned(),
            },
        }
    }

    fn armed() -> NodeBody {
        NodeBody::AudioRecorder(AudioRecorderNode {
            recording: true,
            skip_silence: false,
        })
    }

    fn graph() -> PatchGraph {
        PatchGraph {
            nodes: vec![
                node("day", NodeBody::Schedule(ScheduleNode::default())),
                node("night", NodeBody::Schedule(ScheduleNode::default())),
                node("dev", NodeBody::Device(DeviceNode::default())),
                node("free", NodeBody::Device(DeviceNode::default())),
                node("take", armed()),
            ],
            edges: vec![
                run_wire("day", "dev"),
                run_wire("night", "dev"),
                run_wire("day", "take"),
            ],
        }
    }

    fn waiting(graph: &PatchGraph, id: &str) -> bool {
        matches!(
            graph.node(id).map(|node| &node.body),
            Some(NodeBody::Device(DeviceNode { waiting: true, .. }))
        )
    }

    #[test]
    fn a_wired_radio_rests_until_one_controller_holds_it_on() {
        let gate = RunGate::default();
        let rested = gate.gated(&graph());
        assert!(waiting(&rested, "dev"));
        assert!(!waiting(&rested, "free"), "an unwired radio always runs");
        assert_eq!(
            rested.node("take").map(|node| &node.body),
            Some(&NodeBody::AudioRecorder(AudioRecorderNode::default()))
        );
        gate.hold("night", "dev", true);
        let held = gate.gated(&graph());
        assert!(!waiting(&held, "dev"));
        assert_eq!(
            held.node("take").map(|node| &node.body),
            Some(&NodeBody::AudioRecorder(AudioRecorderNode::default())),
            "a hold on the radio leaves the recorder resting"
        );
    }

    #[test]
    fn a_hold_from_an_unwired_controller_counts_for_nothing() {
        let gate = RunGate::default();
        gate.hold("other", "dev", true);
        assert!(waiting(&gate.gated(&graph()), "dev"));
    }

    #[test]
    fn only_a_change_of_hold_is_announced() {
        let gate = RunGate::default();
        let changes = gate.subscribe();
        gate.hold("day", "take", false);
        assert!(!changes.has_changed().unwrap_or(true));
        gate.hold("day", "take", true);
        gate.hold("day", "take", true);
        assert_eq!(*changes.borrow(), 1);
        assert_eq!(
            gate.gated(&graph()).node("take").map(|node| &node.body),
            Some(&armed())
        );
        gate.retain(|node| node != "day");
        assert_eq!(
            gate.gated(&graph()).node("take").map(|node| &node.body),
            Some(&NodeBody::AudioRecorder(AudioRecorderNode::default()))
        );
    }
}

use sdrmm_wire::{
    NodeBody, PatchGraph, PatchRefusal, StateSnapshot, TimeMachineAction, TimeMachineNode,
};

use crate::{AppState, resolved::Resolved, workspace};

pub(crate) fn release(
    state: &AppState,
    before: &PatchGraph,
    after: &PatchGraph,
) -> Vec<PatchRefusal> {
    let snapshot = state.engine.snapshot();
    let mut refused = close_dropped_radios(state, &snapshot, before, after);
    refused.extend(stop_dropped_captures(state, &snapshot, before, after));
    refused.extend(close_dropped_channels(state, &snapshot, before, after));
    if !before.same_topology(after) {
        crate::array::release(state, &Resolved::of(after.clone(), &snapshot));
    }
    for refusal in &refused {
        tracing::warn!(
            node = refusal.node,
            reason = refusal.reason,
            "released badly"
        );
    }
    refused
}

fn close_dropped_channels(
    state: &AppState,
    snapshot: &StateSnapshot,
    before: &PatchGraph,
    after: &PatchGraph,
) -> Vec<PatchRefusal> {
    let dropped = |node: &str| {
        before
            .node(node)
            .is_some_and(|held| matches!(held.body, NodeBody::Channel(_)))
            && after.node(node).is_none()
    };
    let mut refused = Vec::new();
    for set in &snapshot.device_sets {
        for channel in &set.channels {
            let Some(node) = channel.node.as_deref().filter(|node| dropped(node)) else {
                continue;
            };
            if let Err(err) = state.engine.remove_channel(set.id, channel.id) {
                refused.push(PatchRefusal {
                    node: node.to_owned(),
                    reason: format!("could not close its decoder: {err}"),
                });
            }
        }
    }
    refused
}

fn close_dropped_radios(
    state: &AppState,
    snapshot: &StateSnapshot,
    before: &PatchGraph,
    after: &PatchGraph,
) -> Vec<PatchRefusal> {
    let mut refused = Vec::new();
    for (node, device_set) in workspace::bind_devices(before, snapshot) {
        let Some(set) = snapshot.device_sets.iter().find(|set| set.id == device_set) else {
            continue;
        };
        let held = after
            .node(&node)
            .and_then(|kept| kept.body.device_ref(&kept.id))
            .is_some_and(|reference| reference.matches(&set.device));
        if held {
            continue;
        }
        if let Err(err) = state.engine.remove_device_set(device_set) {
            refused.push(PatchRefusal {
                node,
                reason: format!("could not close its radio: {err}"),
            });
        }
    }
    refused
}

fn stop_dropped_captures(
    state: &AppState,
    snapshot: &StateSnapshot,
    before: &PatchGraph,
    after: &PatchGraph,
) -> Vec<PatchRefusal> {
    let dropped = |node: &str| before.node(node).is_some() && after.node(node).is_none();
    let engine = &state.engine;
    let mut refused = Vec::new();
    let mut note = |node: &str, result: Result<(), sdrmm_engine::EngineError>| {
        if let Err(err) = result {
            refused.push(PatchRefusal {
                node: node.to_owned(),
                reason: format!("could not stop: {err}"),
            });
        }
    };
    for set in &snapshot.device_sets {
        if let Some(export) = set.network_export.as_ref().filter(|e| dropped(&e.node)) {
            note(
                &export.node,
                engine.stop_network_export(set.id, &export.node).map(drop),
            );
        }
        if let Some(history) = set.time_machine.as_ref().filter(|h| dropped(&h.node)) {
            let stopped = engine.control_time_machine(
                set.id,
                history.node.clone(),
                history.stream,
                TimeMachineAction::Disarm,
                TimeMachineNode::default(),
            );
            note(&history.node, stopped.map(drop));
        }
        for channel in &set.channels {
            if let Some(export) = channel.network_export.as_ref().filter(|e| dropped(&e.node)) {
                let stopped = engine.stop_channel_network_export(set.id, channel.id, &export.node);
                note(&export.node, stopped.map(drop));
            }
        }
    }
    refused
}

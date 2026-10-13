use sdrmm_wire::{
    ChannelAddr, DeviceInfo, NodeBody, NodeProblem, PatchApplyReport, PatchBinding, PatchGraph,
    PatchNode, PatchRefusal, ProblemSource, StateSnapshot, WorkspaceSnapshot, WorkspaceState,
};

mod release;

pub(crate) use release::release;

use crate::{
    AppState, array, radar,
    resolved::Resolved,
    rest::AppError,
    workspace::{self, Restored},
};

pub(crate) fn bring_up_active(state: &AppState, id: i64) -> Result<PatchApplyReport, AppError> {
    let workspace = state.store.workspace(id)?;
    let saved = state.store.workspace_state(id)?;
    bring_up(state, id, &workspace.snapshot, &saved)
}

pub(crate) fn bring_up(
    app: &AppState,
    workspace: i64,
    snapshot: &WorkspaceSnapshot,
    saved: &WorkspaceState,
) -> Result<PatchApplyReport, AppError> {
    let mut report = PatchApplyReport::default();
    forget_closed_bindings(app, &app.engine.snapshot());
    restore_bound(app, workspace, &snapshot.graph, saved, &mut report);
    open_radios(app, workspace, &snapshot.graph, saved, &mut report);
    crate::placement::settle_workspace(app, &snapshot.graph, saved, &mut report);
    let resolved = Resolved::of(snapshot.graph.clone(), &app.engine.snapshot());
    report.closed += close_unowned(app, &resolved, &report.bound);
    for (node, reason) in hooks(app, &resolved, saved) {
        report.refused.push(PatchRefusal { node, reason });
    }
    crate::placement::settle_lanes(app, &snapshot.graph, saved, &mut report);
    publish_refusals(app, &report);
    app.resolve_now();
    Ok(report)
}

fn hooks(state: &AppState, resolved: &Resolved, saved: &WorkspaceState) -> Vec<(String, String)> {
    let mut refusals = array::reconcile(state, resolved, saved);
    refusals.extend(radar::reconcile(state, resolved));
    refusals
}

fn publish_refusals(app: &AppState, report: &PatchApplyReport) {
    let problems: Vec<NodeProblem> = report
        .refused
        .iter()
        .map(|refusal| NodeProblem {
            node: refusal.node.clone(),
            source: ProblemSource::Apply,
            text: refusal.reason.clone(),
        })
        .collect();
    app.problems
        .replace(&app.engine, ProblemSource::Apply, &problems);
}

fn restore_bound(
    app: &AppState,
    workspace: i64,
    graph: &PatchGraph,
    saved: &WorkspaceState,
    report: &mut PatchApplyReport,
) {
    for (node, device_set) in workspace::bind_devices(graph, &app.engine.snapshot()) {
        if first_binding(app, workspace, &node, device_set) {
            restore(app, &node, device_set, saved, report);
        }
        report.bound.push(PatchBinding { node, device_set });
    }
}

fn restore(
    app: &AppState,
    node: &str,
    device_set: u32,
    saved: &WorkspaceState,
    report: &mut PatchApplyReport,
) {
    match workspace::restore_device(&app.engine, &app.store, device_set, node, saved) {
        Ok(whole) => note_restore(app, node, whole == Restored::Whole),
        Err(reason) => {
            note_restore(app, node, false);
            report.refused.push(PatchRefusal {
                node: node.to_owned(),
                reason,
            });
        }
    }
}

fn open_radios(
    app: &AppState,
    workspace: i64,
    graph: &PatchGraph,
    saved: &WorkspaceState,
    report: &mut PatchApplyReport,
) {
    let mut attached: Option<Vec<DeviceInfo>> = None;
    for node in graph.device_nodes() {
        if report.bound.iter().any(|bound| bound.node == node.id) {
            continue;
        }
        let Some(device_id) = openable(app, node, &mut attached) else {
            if node.body.device_ref(&node.id).is_some() {
                report.absent.push(node.id.clone());
            }
            continue;
        };
        match app.engine.create_device_set(&device_id) {
            Ok(id) => {
                report.opened += 1;
                first_binding(app, workspace, &node.id, id);
                restore(app, &node.id, id, saved, report);
                report.bound.push(PatchBinding {
                    node: node.id.clone(),
                    device_set: id,
                });
            }
            Err(err) => report.refused.push(PatchRefusal {
                node: node.id.clone(),
                reason: err.to_string(),
            }),
        }
    }
}

fn openable(
    app: &AppState,
    node: &PatchNode,
    attached: &mut Option<Vec<DeviceInfo>>,
) -> Option<String> {
    let reference = node.body.device_ref(&node.id)?;
    let devices = attached.get_or_insert_with(|| app.engine.probe_devices());
    if !devices.iter().any(|info| reference.matches(info))
        && let Some(key) = &reference.key
        && let Some(adopted) = app
            .engine
            .adopt_device(&format!("{}:{key}", reference.backend))
    {
        devices.push(adopted);
    }
    let state = app.engine.snapshot();
    devices
        .iter()
        .filter(|info| reference.matches(info))
        .find(|info| {
            !state
                .device_sets
                .iter()
                .any(|set| set.device.id() == info.id())
        })
        .map(DeviceInfo::id)
}

fn close_unowned(app: &AppState, resolved: &Resolved, bound: &[PatchBinding]) -> u32 {
    let state = app.engine.snapshot();
    let mut closed = 0;
    for binding in bound {
        let Some(set) = state
            .device_sets
            .iter()
            .find(|set| set.id == binding.device_set)
        else {
            continue;
        };
        let cut = set.channels.iter().filter(|channel| {
            channel
                .node
                .as_deref()
                .is_some_and(|owner| decoder_or_gone(&resolved.graph, owner))
                && resolved
                    .patch
                    .owner_of(ChannelAddr {
                        device_set: set.id,
                        channel: channel.id,
                    })
                    .is_none()
        });
        for channel in cut {
            if let Err(err) = app.engine.remove_channel(set.id, channel.id) {
                tracing::warn!(set = set.id, channel = channel.id, %err, "could not close an unwired decoder");
            } else {
                tracing::info!(set = set.id, channel = channel.id, node = ?channel.node, "closed an unwired decoder");
                closed += 1;
            }
        }
    }
    closed
}

fn decoder_or_gone(graph: &PatchGraph, owner: &str) -> bool {
    graph
        .node(owner)
        .is_none_or(|node| matches!(node.body, NodeBody::Channel(_)))
}

pub(crate) fn first_binding(app: &AppState, workspace: i64, node: &str, device_set: u32) -> bool {
    app.restored
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert((workspace, node.to_string(), device_set))
}

fn note_restore(app: &AppState, node: &str, restored: bool) {
    let mut unrestored = app
        .unrestored
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    unrestored.retain(|held| held != node);
    if !restored {
        unrestored.push(node.to_string());
    }
}

fn forget_closed_bindings(app: &AppState, state: &StateSnapshot) {
    app.restored
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|(_, _, set)| state.device_sets.iter().any(|live| live.id == *set));
}

use std::{
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use sdrmm_wire::{
    ControlAck, ControlCommand, ControlVerb, HuntAction, NodeBody, NodeProblem, PatchNode,
    ProblemSource, ScanAction, StateScope, StateSnapshot, SweepJob, SweepStatus,
    UpdateWorkspaceRequest,
};
use tokio::runtime::Handle;

use crate::{
    AppState, control,
    rest::{AppError, active_workspace, lock_gate},
};

/// How often a sweep waiting for its decoder is tried again.
const RECHECK: Duration = Duration::from_secs(2);

/// Holds the operator's start or stop and the server's resume apart, so neither lands halfway
/// through the other.
#[derive(Default)]
pub(crate) struct SweepGate(Mutex<()>);

impl SweepGate {
    pub(crate) fn hold(&self) -> MutexGuard<'_, ()> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The control a scanner or hunt node sends for `action`, None for any other pair.
pub(crate) fn command(action: sdrmm_wire::NodeAction, body: &NodeBody) -> Option<ControlCommand> {
    use sdrmm_wire::NodeAction;
    let job = match (action, body) {
        (NodeAction::Scan(action), NodeBody::Scanner(scanner)) => SweepJob::Scan {
            settings: scanner.settings.clone(),
            action,
        },
        (NodeAction::Hunt(action), NodeBody::Hunt(hunt)) => SweepJob::Hunt {
            params: hunt.sweep,
            action,
        },
        _ => return None,
    };
    Some(ControlCommand::Sweep(job))
}

/// Keeps in the node what the operator did: whether it runs, and the scan's lockouts.
pub(crate) fn remember(
    state: &AppState,
    node: &str,
    command: &ControlCommand,
    acks: &[ControlAck],
    author: Option<&str>,
) -> Result<(), AppError> {
    let ControlCommand::Sweep(job) = command else {
        return Ok(());
    };
    let taken = acks.iter().any(|ack| ack.error.is_none());
    let running = match control::runs(job) {
        Some(true) if taken => Some(true),
        Some(false) => Some(false),
        _ => None,
    };
    let lockouts = lockouts(acks);
    edit_node(state, node, author, |body| {
        kept(body, running, lockouts.as_deref())
    })
}

fn lockouts(acks: &[ControlAck]) -> Option<Vec<f64>> {
    acks.iter().find_map(|ack| match ack.sweep.as_deref() {
        Some(SweepStatus::Scan(status)) => Some(status.settings.lockouts.clone()),
        _ => None,
    })
}

fn kept(body: &mut NodeBody, running: Option<bool>, lockouts: Option<&[f64]>) -> bool {
    let before = body.clone();
    match body {
        NodeBody::Scanner(scanner) => {
            if let Some(on) = running {
                scanner.running = on;
            }
            if let Some(held) = lockouts {
                held.clone_into(&mut scanner.settings.lockouts);
            }
        }
        NodeBody::Hunt(hunt) => {
            if let Some(on) = running {
                hunt.running = on;
            }
        }
        _ => {}
    }
    *body != before
}

fn edit_node(
    state: &AppState,
    node: &str,
    author: Option<&str>,
    change: impl FnOnce(&mut NodeBody) -> bool,
) -> Result<(), AppError> {
    {
        let _serialized = lock_gate(&state.apply_gate);
        let mut active = active_workspace(&state.store)?;
        let graph = &mut active.snapshot.graph;
        let Some(held) = graph.nodes.iter_mut().find(|held| held.id == node) else {
            return Ok(());
        };
        if !change(&mut held.body) {
            return Ok(());
        }
        let request = UpdateWorkspaceRequest {
            revision: active.info.revision,
            name: None,
            snapshot: Some(active.snapshot),
        };
        state
            .store
            .update_workspace(active.info.id, &request, author)?;
    }
    state.engine.emit_scope(StateScope::Workspaces);
    state.resolve_now();
    Ok(())
}

/// Starts every scan and hunt the patch says runs and stops the rest, whenever the patch moves
/// and while a decoder it waits for comes up.
pub(crate) fn follow(state: &AppState) {
    crate::resolved::follow(state, "sweeps", settle);
    let Ok(runtime) = Handle::try_current() else {
        return;
    };
    let state = state.clone();
    runtime.spawn(async move {
        let mut tick = tokio::time::interval(RECHECK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let app = state.clone();
            if tokio::task::spawn_blocking(move || settle(&app))
                .await
                .is_err()
            {
                return;
            }
        }
    });
}

pub(crate) fn settle(state: &AppState) {
    let _held = state.sweeps.hold();
    let resolved = state.resolved.current();
    let live = state.engine.snapshot();
    let problems: Vec<NodeProblem> = resolved
        .graph
        .nodes
        .iter()
        .filter_map(|node| {
            let on = running(&node.body)?;
            if on == sweeping(&live, node) {
                return None;
            }
            if !control::takes(&resolved.patch, &node.id, ControlVerb::Sweep) {
                return None;
            }
            let command = command(action(&node.body, on)?, &node.body)?;
            let acks = state
                .control
                .deliver(&state.engine, &resolved.patch, &node.id, &command);
            fault(&acks).map(|text| NodeProblem {
                node: node.id.clone(),
                source: ProblemSource::Sweep,
                text,
            })
        })
        .collect();
    state
        .problems
        .replace(&state.engine, ProblemSource::Sweep, &problems);
}

const fn running(body: &NodeBody) -> Option<bool> {
    match body {
        NodeBody::Scanner(scanner) => Some(scanner.running),
        NodeBody::Hunt(hunt) => Some(hunt.running),
        _ => None,
    }
}

const fn action(body: &NodeBody, on: bool) -> Option<sdrmm_wire::NodeAction> {
    use sdrmm_wire::NodeAction;
    match (body, on) {
        (NodeBody::Scanner(_), true) => Some(NodeAction::Scan(ScanAction::Start)),
        (NodeBody::Scanner(_), false) => Some(NodeAction::Scan(ScanAction::Stop)),
        (NodeBody::Hunt(_), true) => Some(NodeAction::Hunt(HuntAction::Start)),
        (NodeBody::Hunt(_), false) => Some(NodeAction::Hunt(HuntAction::Stop)),
        _ => None,
    }
}

fn sweeping(live: &StateSnapshot, node: &PatchNode) -> bool {
    let mine = |from: Option<&String>| from.is_some_and(|from| *from == node.id);
    live.device_sets.iter().any(|set| match node.body {
        NodeBody::Scanner(_) => set
            .scanners
            .iter()
            .any(|scan| mine(scan.settings.node.as_ref())),
        NodeBody::Hunt(_) => set
            .hunts
            .iter()
            .any(|hunt| mine(hunt.settings.node.as_ref())),
        _ => false,
    })
}

/// A real refusal, not a decoder or lane that is still on its way.
fn fault(acks: &[ControlAck]) -> Option<String> {
    acks.iter()
        .filter_map(|ack| ack.error.as_ref())
        .find(|error| !control::waiting(error))
        .cloned()
}

#[cfg(test)]
mod tests;

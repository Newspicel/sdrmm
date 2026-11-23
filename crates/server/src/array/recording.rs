use std::time::Duration;

use sdrmm_wire::{ArrayRecordingRequest, NodeBody, NodeProblem, ProblemSource};
use tokio::runtime::Handle;

use crate::{
    AppState,
    rest::{AppError, active_workspace, lock_gate},
};

/// How often an array set recording is checked for a take to start.
const RECHECK: Duration = Duration::from_secs(2);

/// Keeps whether the array records in the active workspace's state.
pub(super) fn remember(state: &AppState, node: &str, on: bool) -> Result<(), AppError> {
    let _serialized = lock_gate(&state.apply_gate);
    let active = active_workspace(&state.store)?;
    let mut saved = active.state;
    if saved.set_array_recording(node, on) {
        state.store.put_workspace_state(active.info.id, &saved)?;
    }
    Ok(())
}

/// Starts a take for every array the operator left recording, after a restart or a switch and
/// once the array runs.
pub(crate) fn follow(state: &AppState) {
    crate::resolved::follow(state, "array recordings", settle);
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
    let Ok(active) = active_workspace(&state.store) else {
        return;
    };
    let idle: Vec<String> = state
        .engine
        .array_statuses()
        .into_iter()
        .filter(|status| status.recording.is_none())
        .map(|status| status.node)
        .collect();
    let problems: Vec<NodeProblem> = active
        .snapshot
        .graph
        .nodes
        .iter()
        .filter(|node| matches!(node.body, NodeBody::Array(_)))
        .filter(|node| active.state.records_array(&node.id) && idle.contains(&node.id))
        .filter_map(|node| {
            let started = state
                .engine
                .start_array_recording(&node.id, &ArrayRecordingRequest::default());
            started.err().map(|error| NodeProblem {
                node: node.id.clone(),
                source: ProblemSource::ArrayRecorder,
                text: error.to_string(),
            })
        })
        .collect();
    state
        .problems
        .replace(&state.engine, ProblemSource::ArrayRecorder, &problems);
}

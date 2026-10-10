use sdrmm_wire::{PatchGraph, ServerEvent, StateScope, StateSnapshot};
use tokio::sync::broadcast::error::RecvError;

use crate::{AppState, rest, workspace};

/// Binds the active workspace's radios as they turn up: a network radio found by a search that
/// finished after the page loaded, or a radio plugged in later.
pub(crate) async fn run(state: AppState) {
    let mut events = state.engine.subscribe_events();
    loop {
        match events.recv().await {
            Ok(ServerEvent::StateChanged {
                scope: StateScope::Devices,
            })
            | Err(RecvError::Lagged(_)) => bind_new_radios(state.clone()).await,
            Ok(_) => {}
            Err(RecvError::Closed) => break,
        }
    }
}

async fn bind_new_radios(state: AppState) {
    let bound = tokio::task::spawn_blocking(move || {
        let Some(id) = state.store.active_workspace_id()? else {
            return Ok(false);
        };
        let graph = state.store.workspace(id)?.snapshot.graph;
        if !waits_for_a_radio(&graph, &state.engine.snapshot()) {
            return Ok(false);
        }
        let _serialized = rest::lock_gate(&state.apply_gate);
        rest::bring_up_active(&state, id).map(|report| report.opened > 0)
    })
    .await;
    match bound {
        Ok(Ok(true)) => tracing::info!("bound a radio the active workspace was waiting for"),
        Ok(Ok(false)) => {}
        Ok(Err(error)) => tracing::warn!(?error, "binding a radio that turned up failed"),
        Err(error) => tracing::warn!(%error, "binding a radio that turned up stopped"),
    }
}

pub(crate) fn waits_for_a_radio(graph: &PatchGraph, state: &StateSnapshot) -> bool {
    let bound = workspace::bind_devices(graph, state);
    graph.device_nodes().any(|node| {
        node.body.device_ref(&node.id).is_some() && !bound.iter().any(|(id, _)| *id == node.id)
    })
}

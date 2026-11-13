use sdrmm_wire::{PatchGraph, ServerEvent, StateScope, StateSnapshot};
use tokio::sync::broadcast::error::RecvError;

use crate::{AppState, rest, rest::AppError, workspace};

/// The active workspace and its revision when the server started.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Saved {
    id: i64,
    revision: u64,
}

impl Saved {
    pub(crate) fn active(state: &AppState) -> Option<Self> {
        let active = state.store.active_workspace();
        if let Err(error) = &active {
            tracing::warn!(
                ?error,
                "could not read the active workspace to open its radios"
            );
        }
        active.ok().flatten().map(|detail| Self {
            id: detail.info.id,
            revision: detail.info.revision,
        })
    }
}

/// Binds the active workspace's radios as they turn up: a network radio found by a search that
/// finished after the page loaded, or a radio plugged in later.
pub(crate) async fn run(state: AppState, saved: Option<Saved>) {
    let mut events = state.engine.subscribe_events();
    bring_up_saved(state.clone(), saved).await;
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

/// Opens the workspace saved at startup. Once a client has switched or edited it, that client
/// applies it, so a late startup pass leaves it alone.
async fn bring_up_saved(state: AppState, saved: Option<Saved>) {
    let Some(saved) = saved else {
        return;
    };
    let opened = tokio::task::spawn_blocking(move || {
        let _serialized = rest::lock_gate(&state.apply_gate);
        if Saved::active(&state) != Some(saved) {
            return Ok(false);
        }
        crate::apply::bring_up_active(&state, saved.id).map(|report| report.opened > 0)
    })
    .await;
    match opened {
        Ok(Ok(true)) => tracing::info!("opened the active workspace's radios"),
        Ok(Ok(false)) => {}
        Ok(Err(error)) => tracing::warn!(?error, "opening the active workspace's radios failed"),
        Err(error) => tracing::warn!(%error, "opening the active workspace's radios stopped"),
    }
}

/// The resolved patch only says whether binding is worth the gate. Which workspace to bring up
/// is read under the gate, so a switch that finished while this waited is not undone.
pub(crate) async fn bind_new_radios(state: AppState) {
    let bound = tokio::task::spawn_blocking(move || -> Result<bool, AppError> {
        let resolved = state.resolved.current();
        if resolved.patch.workspace.is_none()
            || !waits_for_a_radio(&resolved.graph, &state.engine.snapshot())
        {
            return Ok(false);
        }
        let _serialized = rest::lock_gate(&state.apply_gate);
        let Some(id) = state.store.active_workspace_id()? else {
            return Ok(false);
        };
        crate::apply::bring_up_active(&state, id).map(|report| report.opened > 0)
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

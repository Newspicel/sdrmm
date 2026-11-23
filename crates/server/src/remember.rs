use sdrmm_wire::{NodeBody, StateScope, UpdateWorkspaceRequest};

use crate::{
    AppState,
    rest::{AppError, active_workspace, lock_gate},
};

/// Keeps what the operator did in one node of the active workspace. `change` says whether it
/// changed anything; a node the workspace no longer has is left alone.
pub(crate) fn edit_node(
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

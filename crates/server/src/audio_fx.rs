use std::{
    collections::HashSet,
    sync::{Mutex, PoisonError},
};

use sdrmm_engine::Engine;
use sdrmm_wire::{NodeBody, PatchGraph};

use crate::{
    AppState,
    recorders::{Hooks, Recorders},
    resolved::{self, ResolvedHub},
};

pub(crate) fn follow(state: &AppState) {
    let hooks = Hooks {
        store: state.store.clone(),
        gate: state.recordings_gate.clone(),
        gps: state.gps.clone(),
        problems: state.problems.clone(),
    };
    let recorders = Mutex::new(Recorders::default());
    resolved::follow(state, "audio FX and recorders", move |state| {
        let current = state.resolved.current();
        apply(&state.engine, &current.graph);
        recorders
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .reconcile(&state.engine, &current, &hooks);
    });
}

pub(crate) fn sync(engine: &Engine, resolved: &ResolvedHub) {
    apply(engine, &resolved.current().graph);
}

fn apply(engine: &Engine, graph: &PatchGraph) {
    let mut present = HashSet::new();
    for node in &graph.nodes {
        let NodeBody::AudioFx(fx) = &node.body else {
            continue;
        };
        match engine.set_audio_fx(&node.id, fx.settings.clone()) {
            Ok(()) => {
                present.insert(node.id.clone());
            }
            Err(error) => tracing::warn!(node = %node.id, %error, "audio FX settings refused"),
        }
    }
    engine.retain_audio_fx(&present);
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{AudioFxNode, AudioRoute, PatchNode, Position};

    use super::*;

    fn fx(id: &str) -> PatchNode {
        PatchNode {
            id: id.to_owned(),
            body: NodeBody::AudioFx(AudioFxNode::default()),
            position: Position { x: 0.0, y: 0.0 },
            size: None,
            label: None,
        }
    }

    #[test]
    fn only_the_fx_nodes_in_the_patch_can_be_routed_through() {
        let engine = Engine::new(None);
        apply(
            &engine,
            &PatchGraph {
                nodes: vec![fx("kept")],
                edges: Vec::new(),
            },
        );
        let through = |node: &str| AudioRoute {
            fx: vec![node.to_owned()],
            ..AudioRoute::channel(0, 0)
        };
        let unknown = engine.subscribe_route_pcm(&through("gone")).unwrap_err();
        assert!(
            unknown.to_string().contains("not in the patch"),
            "{unknown}"
        );
        let known = engine.subscribe_route_pcm(&through("kept")).unwrap_err();
        assert!(known.is_not_found(), "{known}");
    }

    #[tokio::test]
    async fn an_fx_node_saved_on_the_canvas_is_live_without_a_poll() {
        let (_app, state) = crate::tests::test_router_with_state();
        let workspace = state
            .store
            .active_workspace_id()
            .expect("store")
            .expect("seeded workspace");
        let detail = state.store.workspace(workspace).expect("workspace");
        let mut snapshot = detail.snapshot;
        snapshot.graph.nodes.push(fx("kept"));
        state
            .store
            .update_workspace(
                workspace,
                &sdrmm_wire::UpdateWorkspaceRequest {
                    revision: detail.info.revision,
                    name: None,
                    snapshot: Some(snapshot),
                },
                None,
            )
            .expect("save");
        state.resolve_now();
        let through = AudioRoute {
            fx: vec!["kept".to_owned()],
            ..AudioRoute::channel(0, 0)
        };
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let refused = state.engine.subscribe_route_pcm(&through).unwrap_err();
            if refused.is_not_found() {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the FX node never went live: {refused}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
}

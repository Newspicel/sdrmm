use std::{
    collections::HashSet,
    sync::{Mutex, PoisonError},
};

use sdrmm_engine::Engine;
use sdrmm_wire::{AudioRoute, ChannelAddr, NodeBody, PatchGraph, PortType, ResolvedPatch, Tap};

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
        state.engine.retain_voices(&voices(&current.patch));
        recorders
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .reconcile(&state.engine, &current, &hooks);
    });
}

/// The nodes whose voice some wire listens to.
fn voices(patch: &ResolvedPatch) -> HashSet<String> {
    patch
        .nodes
        .iter()
        .flat_map(|node| &node.ports)
        .flat_map(|port| &port.feeds)
        .filter_map(|feed| match &feed.tap {
            Some(Tap::Node { node, .. }) if feed.payload.port_type() == PortType::Audio => {
                Some(node.clone())
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn sync(engine: &Engine, resolved: &ResolvedHub) {
    apply(engine, &resolved.current().graph);
}

fn apply(engine: &Engine, graph: &PatchGraph) {
    let mut present = HashSet::new();
    for node in &graph.nodes {
        let NodeBody::Channel(channel) = &node.body else {
            continue;
        };
        if !channel.audio.is_active() {
            continue;
        }
        match engine.set_audio_fx(&node.id, channel.audio.clone()) {
            Ok(()) => {
                present.insert(node.id.clone());
            }
            Err(error) => tracing::warn!(node = %node.id, %error, "audio FX settings refused"),
        }
    }
    engine.retain_audio_fx(&present);
}

/// The sound a video wire from `source` carries with its pictures: the channel's own audio,
/// through its audio chain, when its decoder makes sound.
pub(crate) fn video_sound(graph: &PatchGraph, source: &str, at: ChannelAddr) -> Option<AudioRoute> {
    let Some(NodeBody::Channel(channel)) = graph.node(source).map(|node| &node.body) else {
        return None;
    };
    sdrmm_decoder::registry::descriptor(&channel.channel_type)
        .is_some_and(|descriptor| descriptor.has_audio && descriptor.has_video)
        .then(|| AudioRoute::channel(at.device_set, at.channel).through(graph.audio_fx(source)))
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{AudioProcessing, AudioRoute, ChannelNode, NodeKind, PatchNode, Position};

    use super::*;

    fn fx(id: &str) -> PatchNode {
        PatchNode {
            id: id.to_owned(),
            body: NodeBody::Channel(ChannelNode {
                channel_type: "nfm".to_owned(),
                tuning_locked: false,
                audio: AudioProcessing {
                    auto_notch: true,
                    ..AudioProcessing::default()
                },
            }),
            position: Position { x: 0.0, y: 0.0 },
            size: None,
            label: None,
        }
    }

    #[test]
    fn only_decoders_with_audio_on_can_be_routed_through() {
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
    async fn decoder_audio_saved_on_the_canvas_is_live_without_a_poll() {
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
                "the decoder's audio chain never went live: {refused}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    fn graph(id: &str, body: NodeBody) -> PatchGraph {
        PatchGraph {
            nodes: vec![PatchNode {
                id: id.to_owned(),
                body,
                position: Position { x: 0.0, y: 0.0 },
                size: None,
                label: None,
            }],
            edges: Vec::new(),
        }
    }

    fn channel(channel_type: &str) -> NodeBody {
        NodeBody::Channel(ChannelNode {
            channel_type: channel_type.to_owned(),
            ..ChannelNode::default()
        })
    }

    #[test]
    fn a_video_wire_carries_the_sound_of_a_decoder_that_makes_both() {
        sdrmm_channels::builtin::install();
        let at = ChannelAddr {
            device_set: 2,
            channel: 5,
        };
        assert_eq!(
            video_sound(&graph("tv", channel("dvbt")), "tv", at),
            Some(AudioRoute::channel(2, 5))
        );
        assert_eq!(video_sound(&graph("fm", channel("wfm")), "fm", at), None);
        assert_eq!(
            video_sound(
                &graph("screen", NodeBody::default_for(NodeKind::Video)),
                "screen",
                at
            ),
            None
        );
    }
}

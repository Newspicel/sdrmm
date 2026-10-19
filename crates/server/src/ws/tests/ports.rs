use sdrmm_wire::{
    AudioRoute, ChannelAddr, NodeKind, PatchEdge, PatchGraph, PatchNode, ResolvedPatch, Runs, Tap,
    UpdateWorkspaceRequest, WorkspaceSnapshot,
    patch::resolve::{ResolveCtx, compile},
};

use super::*;
use crate::ws::ports::{Tapped, Upstream, Wanted, wanted};

fn compiled(nodes: Vec<PatchNode>, edges: Vec<PatchEdge>) -> ResolvedPatch {
    compile(&PatchGraph { nodes, edges }, &ResolveCtx::default()).expect("compiles")
}

fn tap_all(patch: &mut ResolvedPatch, tap: &Tap) {
    for node in &mut patch.nodes {
        for port in &mut node.ports {
            for feed in &mut port.feeds {
                feed.tap = Some(tap.clone());
            }
        }
    }
}

const fn addr(channel: u32) -> ChannelAddr {
    ChannelAddr {
        device_set: 1,
        channel,
    }
}

#[test]
fn an_unwired_port_is_refused() {
    let patch = compiled(vec![of("spk", NodeKind::Speaker)], Vec::new());
    assert_eq!(wanted(&patch, "spk", "audio"), Err("Not wired".to_owned()));
    assert_eq!(
        wanted(&patch, "ghost", "audio"),
        Err("No such node".to_owned())
    );
}

#[test]
fn the_fx_chain_comes_from_via() {
    let mut patch = compiled(
        vec![
            channel("voice", "nfm"),
            of("fx", NodeKind::AudioFx),
            of("spk", NodeKind::Speaker),
        ],
        vec![
            wire(("voice", "audio"), ("fx", "audio")),
            wire(("fx", "audio"), ("spk", "audio")),
        ],
    );
    assert_eq!(
        wanted(&patch, "spk", "audio"),
        Err("Not running".to_owned())
    );
    tap_all(
        &mut patch,
        &Tap::Channel {
            device_set: 1,
            channel: 4,
        },
    );
    let route = AudioRoute {
        device_set: 1,
        channel: 4,
        fx: vec!["fx".to_owned()],
    };
    let from = Upstream {
        source: "voice".to_owned(),
        via: vec!["fx".to_owned()],
        index: 0,
    };
    assert_eq!(
        wanted(&patch, "spk", "audio"),
        Ok(vec![
            Wanted {
                tapped: Tapped::Audio(route.clone()),
                from: from.clone(),
            },
            Wanted {
                tapped: Tapped::AudioSpectrum(route),
                from,
            },
        ])
    );
}

#[test]
fn a_trunk_feed_gives_one_stream_per_follower() {
    let mut patch = compiled(
        vec![
            of("trunk", NodeKind::DmrTrunk),
            of("spk", NodeKind::Speaker),
        ],
        vec![wire(("trunk", "audio"), ("spk", "audio"))],
    );
    tap_all(
        &mut patch,
        &Tap::Channels {
            channels: vec![addr(2), addr(3)],
        },
    );
    let streams = wanted(&patch, "spk", "audio").expect("streams");
    let sources: Vec<(&str, u32)> = streams
        .iter()
        .filter(|want| matches!(want.tapped, Tapped::Audio(_)))
        .map(|want| (want.from.source.as_str(), want.from.index))
        .collect();
    assert_eq!(sources, [("trunk", 0), ("trunk", 1)]);
    assert_eq!(
        streams[2].tapped,
        Tapped::Audio(AudioRoute::channel(1, 3)),
        "each follower is its own stream"
    );
}

#[test]
fn a_device_serves_its_own_second_lane_spectrum() {
    let mut patch = compiled(
        vec![radio("band"), of("scope", NodeKind::Scope)],
        vec![wire(("radio", "iq2"), ("scope", "iq"))],
    );
    assert_eq!(wanted(&patch, "radio", "iq"), Err("Not running".to_owned()));
    for node in &mut patch.nodes {
        node.runs = Runs::Radio { device_set: 7 };
    }
    assert_eq!(
        wanted(&patch, "radio", "iq2"),
        Ok(vec![Wanted {
            tapped: Tapped::Spectrum {
                device_set: 7,
                stream: 1,
            },
            from: Upstream {
                source: "radio".to_owned(),
                ..Upstream::default()
            },
        }])
    );
}

fn speaker_graph(from: Option<&str>) -> PatchGraph {
    let mut edges = vec![
        wire(("radio", "iq"), ("a", "iq")),
        wire(("radio", "iq"), ("b", "iq")),
    ];
    if let Some(from) = from {
        edges.push(wire((from, "audio"), ("spk", "audio")));
    }
    PatchGraph {
        nodes: vec![
            radio("band"),
            channel("a", "nfm"),
            channel("b", "nfm"),
            of("spk", NodeKind::Speaker),
        ],
        edges,
    }
}

async fn subscribe_speaker(ws: &mut WsClient) {
    subscribe_port(ws, "spk", "audio").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unwired_speaker_is_refused() {
    let (mut ws, _state, _) = bench(speaker_graph(None)).await;
    subscribe_speaker(&mut ws).await;
    assert_eq!(
        next_port_event(&mut ws).await,
        ServerEvent::PortRefused {
            node: "spk".to_owned(),
            port: "audio".to_owned(),
            reason: "Not wired".to_owned(),
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn audio_follows_a_rewire_without_a_resubscribe() {
    let (mut ws, state, id) = bench(speaker_graph(Some("a"))).await;
    subscribe_speaker(&mut ws).await;
    let ServerEvent::PortStream {
        stream_id, source, ..
    } = next_port_event(&mut ws).await
    else {
        panic!("the speaker streams channel a");
    };
    assert_eq!(source, "a");
    let revision = state.store.workspace(id).expect("workspace").info.revision;
    let request = UpdateWorkspaceRequest {
        revision,
        name: None,
        snapshot: Some(WorkspaceSnapshot::new(
            speaker_graph(Some("b")),
            sdrmm_wire::RackLayout::default(),
        )),
    };
    state
        .store
        .update_workspace(id, &request, None)
        .expect("rewired");
    apply(&state, id).await;
    assert_eq!(
        next_port_event(&mut ws).await,
        ServerEvent::StreamStopped {
            stream_id,
            kind: StreamKind::Audio,
        }
    );
    let ServerEvent::PortStream { source, kind, .. } = next_port_event(&mut ws).await else {
        panic!("the speaker moves to channel b");
    };
    assert_eq!((source.as_str(), kind), ("b", StreamKind::Audio));
}

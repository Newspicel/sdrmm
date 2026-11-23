//! The rule every wire keeps: a node hears another only through a wire, and every wire the
//! contract admits carries what it promises. A feed of samples, audio or pictures from a running
//! source always names the stream it taps; one that names none would sit drawn and silent.

use super::*;
use crate::{NodeKind, PortType, state::TrunkSystemStatus};

const CARRIED: [PortType; 4] = [
    PortType::Iq,
    PortType::Baseband,
    PortType::Audio,
    PortType::Video,
];
const PROBE: &str = "probe_all";

fn probe() -> Vec<ChannelDescriptor> {
    vec![ChannelDescriptor {
        type_id: PROBE.to_owned(),
        name: PROBE.to_owned(),
        bandwidth_hz: 12_500.0,
        input_rate_hz: 48_000.0,
        has_audio: true,
        has_video: true,
        ..ChannelDescriptor::default()
    }]
}

/// The node `src` of `kind`, running: its nodes, wires and the live state it runs in.
fn running(kind: NodeKind) -> (Vec<PatchNode>, Vec<PatchEdge>, StateSnapshot) {
    match kind {
        NodeKind::Device => (
            vec![radio("src", "band")],
            Vec::new(),
            live(vec![set(1, "band", Vec::new())]),
        ),
        NodeKind::Channel => (
            vec![radio("dev", "band"), channel("src", PROBE)],
            vec![wire(("dev", "iq"), ("src", "iq"))],
            live(vec![set(
                1,
                "band",
                vec![live_channel(7, Some("src"), PROBE, 0)],
            )]),
        ),
        NodeKind::Trunk => {
            let mut state = live(Vec::new());
            state.trunk_systems.push(TrunkSystemStatus {
                node: "src".to_owned(),
                control: Some(crate::state::TrunkControl {
                    device_set: 1,
                    channel: 2,
                    freq_hz: 1,
                }),
                ..TrunkSystemStatus::default()
            });
            (
                vec![node("src", NodeBody::default_for(kind))],
                Vec::new(),
                state,
            )
        }
        _ => (
            vec![node("src", NodeBody::default_for(kind))],
            Vec::new(),
            live(Vec::new()),
        ),
    }
}

fn sink(kind: NodeKind) -> PatchNode {
    match kind {
        NodeKind::Channel => channel("dst", PROBE),
        _ => node("dst", NodeBody::default_for(kind)),
    }
}

fn ports(nodes: Vec<PatchNode>, id: &str, direction: PortDirection) -> Vec<(String, PortType)> {
    let all = probe();
    let ctx = ResolveCtx {
        descriptors: Some(&all),
        live: None,
    };
    let graph = PatchGraph {
        nodes,
        edges: Vec::new(),
    };
    compile_draft(&graph, &ctx)
        .0
        .node(id)
        .map_or_default(|found| {
            found
                .ports
                .iter()
                .filter(|port| port.spec.direction == direction)
                .filter(|port| CARRIED.contains(&port.spec.port_type))
                .map(|port| (port.spec.name.clone(), port.spec.port_type))
                .collect()
        })
}

/// Whether the wire from `src`'s `out` into `dst`'s `input` is admitted and, if so, whether
/// it taps a stream: None when the contract refuses it.
fn delivers(source: NodeKind, out: &str, target: NodeKind, input: &str) -> Option<bool> {
    let (mut nodes, mut edges, state) = running(source);
    nodes.push(sink(target));
    edges.push(wire(("src", out), ("dst", input)));
    let graph = PatchGraph { nodes, edges };
    let all = probe();
    let ctx = ResolveCtx {
        descriptors: Some(&all),
        live: Some(&state),
    };
    let (mut patch, refused) = compile_draft(&graph, &ctx);
    if !refused.is_empty() {
        return None;
    }
    bind(&mut patch, &graph, &state, &[]);
    Some(
        patch
            .feeds("dst", input)
            .iter()
            .any(|feed| feed.source == "src" && feed.tap.is_some()),
    )
}

#[test]
fn every_admitted_wire_carries_its_stream() {
    let mut silent = Vec::new();
    for &source in NodeKind::ALL {
        let (nodes, _, _) = running(source);
        for (out, port_type) in ports(nodes, "src", PortDirection::Out) {
            for &target in NodeKind::ALL {
                for (input, _) in ports(vec![sink(target)], "dst", PortDirection::In)
                    .into_iter()
                    .filter(|(_, taken)| *taken == port_type)
                {
                    if delivers(source, &out, target, &input) == Some(false) {
                        silent.push(format!("{source}.{out} -> {target}.{input}"));
                    }
                }
            }
        }
    }
    assert!(
        silent.is_empty(),
        "admitted wires that carry nothing:\n{}",
        silent.join("\n")
    );
}

#[test]
fn a_microphone_and_a_voice_chat_reach_a_speaker_and_each_other() {
    for (source, target) in [
        (NodeKind::Microphone, NodeKind::Speaker),
        (NodeKind::VoiceChat, NodeKind::Speaker),
        (NodeKind::Microphone, NodeKind::VoiceChat),
        (NodeKind::VoiceChat, NodeKind::VoiceChat),
        (NodeKind::Microphone, NodeKind::StreamOut),
    ] {
        assert_eq!(
            delivers(source, "audio", target, "audio"),
            Some(true),
            "{source} -> {target}"
        );
    }
}

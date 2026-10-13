use sdrmm_device::DeviceRegistry;
use sdrmm_engine::Engine;
use sdrmm_wire::{
    ChannelNode, ChannelParams, ChannelSettings, DeviceRef, NodeBody, PatchEdge, PatchNode,
    PortRef, Position, UpdateWorkspaceRequest, WorkspaceSnapshot,
};

use super::*;
use crate::{Store, problems::ProblemBoard, resolved::ResolvedHub};

struct Bound {
    bindings: Vec<CallBinding>,
    problems: Vec<NodeProblem>,
}

fn resolved(store: &Store, engine: &Engine) -> Bound {
    let hub = ResolvedHub::default();
    hub.reload(store, engine, &ProblemBoard::default());
    let (bound, problems) = bindings(&hub.current().patch, &engine.snapshot());
    Bound {
        bindings: bound.into_values().flatten().collect(),
        problems,
    }
}

fn channel_node(id: &str, channel_type: &str) -> PatchNode {
    PatchNode {
        id: id.to_owned(),
        body: NodeBody::Channel(ChannelNode {
            channel_type: channel_type.to_owned(),
            tuning_locked: false,
        }),
        position: Position { x: 0.0, y: 0.0 },
        size: None,
        label: None,
    }
}

fn live_channel_workspace(
    store: &Store,
    engine: &Engine,
    node: PatchNode,
    squelch: sdrmm_wire::Squelch,
    wired: bool,
) {
    let set = engine
        .create_device_set("virtual:band")
        .expect("open the virtual radio");
    let NodeBody::Channel(channel) = &node.body else {
        panic!("the fixture wires a channel node");
    };
    engine
        .add_channel_for(
            set,
            0,
            ChannelSettings {
                frequency_hz: 100_000_000.0,
                squelch,
                params: ChannelParams::default_for(&channel.channel_type)
                    .expect("a known channel type"),
                blanker: Default::default(),
            },
            Some(&node.id),
        )
        .expect("add channel");
    let mut snapshot = WorkspaceSnapshot::starter();
    let node_id = node.id.clone();
    snapshot.graph.nodes.push(node);
    snapshot.graph.edges.push(PatchEdge {
        from: PortRef {
            node: "device".to_owned(),
            port: "iq".to_owned(),
        },
        to: PortRef {
            node: node_id.clone(),
            port: "iq".to_owned(),
        },
    });
    if wired {
        snapshot.graph.nodes.push(PatchNode {
            id: "log".to_owned(),
            body: NodeBody::Readout,
            position: Position { x: 0.0, y: 0.0 },
            size: None,
            label: None,
        });
        snapshot.graph.edges.push(PatchEdge {
            from: PortRef {
                node: node_id,
                port: "events".to_owned(),
            },
            to: PortRef {
                node: "log".to_owned(),
                port: "events".to_owned(),
            },
        });
    }
    let NodeBody::Device(device) = &mut snapshot
        .graph
        .nodes
        .iter_mut()
        .find(|node| node.id == "device")
        .expect("the starter draws a radio")
        .body
    else {
        panic!("the starter's radio is a device node");
    };
    device.device = Some(DeviceRef {
        backend: "virtual".to_owned(),
        serial: None,
        key: Some("band".to_owned()),
    });
    let id = store.create_workspace("w", &snapshot).expect("workspace");
    store.activate_workspace(id).expect("activate");
}

fn bound_for(channel_type: &str, squelch: sdrmm_wire::Squelch, wired: bool) -> Bound {
    let store = Store::open(None).expect("in-memory store");
    let mut registry = DeviceRegistry::new();
    registry.register(1, Box::new(sdrmm_device_virtual::VirtualDriver::new()));
    let engine = Engine::with_registry(registry, None);
    live_channel_workspace(
        &store,
        &engine,
        channel_node("radio", channel_type),
        squelch,
        wired,
    );
    resolved(&store, &engine)
}

#[test]
fn a_wired_voice_channel_keeps_calls_framed_by_its_protocol() {
    let bound = bound_for("dmr", sdrmm_wire::Squelch::Off, true);
    assert_eq!(bound.bindings.len(), 1);
    assert_eq!(bound.bindings[0].node, "radio");
    assert_eq!(bound.bindings[0].gate, CallGate::Frames);
}

#[test]
fn a_wired_analog_channel_keeps_calls_framed_by_its_squelch() {
    let squelch = sdrmm_wire::Squelch::Manual { level_db: -80.0 };
    let bound = bound_for("am", squelch, true);
    assert_eq!(bound.bindings.len(), 1);
    assert_eq!(bound.bindings[0].gate, CallGate::Squelch);
    assert!(bound.problems.is_empty());
}

#[test]
fn a_squelched_broadcast_channel_makes_no_calls() {
    let squelch = sdrmm_wire::Squelch::Manual { level_db: -80.0 };
    assert!(bound_for("drm", squelch, true).bindings.is_empty());
}

#[test]
fn a_squelched_data_decoder_makes_no_calls() {
    let squelch = sdrmm_wire::Squelch::Manual { level_db: -80.0 };
    assert!(bound_for("pocsag", squelch, true).bindings.is_empty());
}

#[test]
fn an_open_squelch_says_why_no_calls_come() {
    let bound = bound_for("am", sdrmm_wire::Squelch::Off, true);
    assert!(bound.bindings.is_empty());
    assert_eq!(
        bound.problems,
        [NodeProblem {
            node: "radio".to_owned(),
            source: ProblemSource::Calls,
            text: SQUELCH_OFF.to_owned(),
        }]
    );
}

#[test]
fn a_channel_whose_events_reach_no_call_sink_keeps_no_calls() {
    let store = Store::open(None).expect("in-memory store");
    let mut registry = DeviceRegistry::new();
    registry.register(1, Box::new(sdrmm_device_virtual::VirtualDriver::new()));
    let engine = Engine::with_registry(registry, None);
    live_channel_workspace(
        &store,
        &engine,
        channel_node("radio", "dmr"),
        sdrmm_wire::Squelch::Off,
        true,
    );
    let id = store.active_workspace_id().expect("id").expect("active");
    let mut detail = store.workspace(id).expect("detail");
    for node in &mut detail.snapshot.graph.nodes {
        if node.id == "log" {
            node.body = NodeBody::Map;
        }
    }
    store
        .update_workspace(
            id,
            &UpdateWorkspaceRequest {
                revision: detail.info.revision,
                name: None,
                snapshot: Some(detail.snapshot),
            },
            None,
        )
        .expect("a map instead of the readout");
    let bound = resolved(&store, &engine);
    assert!(bound.bindings.is_empty(), "a map takes no calls");
}

#[test]
fn a_channel_wired_only_to_a_readout_keeps_no_call_audio() {
    let bound = bound_for("dmr", sdrmm_wire::Squelch::Off, true);
    assert!(!bound.bindings[0].keep_audio);
}

#[test]
fn a_channel_with_nothing_on_its_events_keeps_nothing() {
    assert!(
        bound_for("dmr", sdrmm_wire::Squelch::Off, false)
            .bindings
            .is_empty()
    );
}

#[test]
fn a_channel_the_radio_never_opened_cannot_record_calls() {
    let store = Store::open(None).expect("in-memory store");
    let engine = Engine::with_registry(DeviceRegistry::new(), None);
    let mut snapshot = WorkspaceSnapshot::empty();
    snapshot.graph.nodes.push(channel_node("dmr", "dmr"));
    let id = store.create_workspace("w", &snapshot).expect("workspace");
    store.activate_workspace(id).expect("activate");

    assert!(resolved(&store, &engine).bindings.is_empty());
}

#[test]
fn a_trunk_binds_each_follower_its_wire_asks_calls_of() {
    let store = Store::open(None).expect("in-memory store");
    let mut registry = DeviceRegistry::new();
    registry.register(1, Box::new(sdrmm_device_virtual::VirtualDriver::new()));
    let engine = Engine::with_registry(registry, None);
    live_channel_workspace(
        &store,
        &engine,
        channel_node("radio", "dmr"),
        sdrmm_wire::Squelch::Off,
        false,
    );
    let live = engine.snapshot();
    let follower = live
        .device_sets
        .iter()
        .flat_map(|set| {
            set.channels.iter().map(|info| ChannelAddr {
                device_set: set.id,
                channel: info.id,
            })
        })
        .next()
        .expect("a live channel");
    let at = |id: &str, body: NodeBody| PatchNode {
        id: id.to_owned(),
        body,
        position: Position { x: 0.0, y: 0.0 },
        size: None,
        label: None,
    };
    let graph = sdrmm_wire::PatchGraph {
        nodes: vec![
            at("trunk", NodeBody::DmrTrunk(Default::default())),
            at("log", NodeBody::DecoderLog(Default::default())),
        ],
        edges: vec![PatchEdge {
            from: PortRef {
                node: "trunk".to_owned(),
                port: "events".to_owned(),
            },
            to: PortRef {
                node: "log".to_owned(),
                port: "events".to_owned(),
            },
        }],
    };
    let mut patch =
        sdrmm_wire::patch::resolve::compile(&graph, &Default::default()).expect("compiles");
    for node in &mut patch.nodes {
        if node.id == "trunk" {
            node.runs = Runs::Trunk {
                control: None,
                followers: vec![follower],
            };
        }
    }

    let (bound, problems) = bindings(&patch, &live);

    assert!(problems.is_empty());
    assert_eq!(
        bound.get(&(follower.device_set, follower.channel)),
        Some(&vec![CallBinding {
            node: "trunk".to_owned(),
            device_set: follower.device_set,
            channel: follower.channel,
            gate: CallGate::Frames,
            keep_audio: true,
        }]),
        "the log keeps the clips its events bring"
    );
}

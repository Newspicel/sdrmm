use std::sync::Arc;

use sdrmm_engine::Engine;
use sdrmm_wire::{
    ChannelNode, ChannelSettings, DeviceBackend, DeviceNode, DeviceRef, GpsNode, NodeBody,
    PassiveRadarNode, PatchEdge, PatchGraph, PatchNode, PortRef, Position, PositionFix,
    PositionSource, RecorderNode, WorkspaceSnapshot,
};

use super::super::routes::{PositionTarget, push};
use super::*;

fn node(id: &str, body: NodeBody) -> PatchNode {
    PatchNode {
        id: id.to_owned(),
        body,
        position: Position { x: 0.0, y: 0.0 },
        size: None,
        label: None,
    }
}

fn wire(from: (&str, &str), to: (&str, &str)) -> PatchEdge {
    PatchEdge {
        from: PortRef {
            node: from.0.to_owned(),
            port: from.1.to_owned(),
        },
        to: PortRef {
            node: to.0.to_owned(),
            port: to.1.to_owned(),
        },
    }
}

fn fixed() -> GpsNode {
    GpsNode {
        source: Some(PositionSource::Fixed {
            lat: 52.0,
            lon: 13.0,
            altitude_m: None,
        }),
    }
}

fn fix() -> PositionFix {
    PositionFix {
        latitude: 52.0,
        longitude: 13.0,
        altitude_m: None,
        accuracy_m: None,
        speed_mps: None,
        track_deg: None,
        time: "2026-10-12T00:00:00Z".to_owned(),
        attitude: sdrmm_wire::Attitude::default(),
    }
}

fn bench() -> (Arc<Engine>, PatchGraph, u32, u32) {
    let mut registry = sdrmm_device::DeviceRegistry::new();
    registry.register(1, Box::new(sdrmm_device_virtual::VirtualDriver::new()));
    let engine = Engine::with_registry(registry, None);
    let set = engine.create_device_set("virtual:band").expect("radio");
    let settings = ChannelSettings::default_for("adsb").expect("adsb");
    let channel = engine
        .add_channel_for(set, 0, settings, Some("planes"))
        .expect("channel");
    let graph = PatchGraph {
        nodes: vec![
            node(
                "dev",
                NodeBody::Device(DeviceNode {
                    device: Some(DeviceRef {
                        backend: DeviceBackend::Virtual,
                        serial: None,
                        key: Some("band".to_owned()),
                    }),
                    ..DeviceNode::default()
                }),
            ),
            node("gps", NodeBody::Gps(fixed())),
            node(
                "planes",
                NodeBody::Channel(ChannelNode {
                    channel_type: "adsb".to_owned(),
                    tuning_locked: false,
                }),
            ),
            node("rec", NodeBody::Recorder(RecorderNode::default())),
            node("radar", NodeBody::PassiveRadar(PassiveRadarNode::default())),
        ],
        edges: vec![
            wire(("dev", "iq"), ("planes", "iq")),
            wire(("dev", "iq"), ("rec", "iq")),
            wire(("gps", "position"), ("planes", "position")),
            wire(("gps", "position"), ("rec", "position")),
            wire(("gps", "position"), ("radar", "tx")),
        ],
    };
    (engine, graph, set, channel)
}

#[test]
fn every_position_input_is_a_route_and_only_decoders_and_recorders_are_pushed() {
    let (engine, graph, set, channel) = bench();
    let routes = PositionRoutes::resolve(&crate::workspace::resolve(&graph, &engine.snapshot()));
    let mut targets = routes.targets("gps").to_vec();
    targets.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        targets,
        [
            (
                "planes".to_owned(),
                PositionTarget::Decoder(sdrmm_wire::ChannelAddr {
                    device_set: set,
                    channel
                })
            ),
            (
                "radar".to_owned(),
                PositionTarget::Sink {
                    node: "radar".to_owned()
                }
            ),
            (
                "rec".to_owned(),
                PositionTarget::IqRecording { device_set: set }
            ),
        ]
    );
}

#[test]
fn a_fix_reaches_a_decoder_and_a_recorder_from_the_routes_alone() {
    let (engine, graph, _, _) = bench();
    let routes = PositionRoutes::resolve(&crate::workspace::resolve(&graph, &engine.snapshot()));
    let problems = ProblemBoard::default();
    push(&engine, &problems, &routes, "gps", Some(&fix()));
    assert!(problems.all().is_empty(), "{:?}", problems.all());
}

#[test]
fn a_fix_that_cannot_land_is_a_problem_on_its_target() {
    let (engine, graph, set, _) = bench();
    let routes = PositionRoutes::resolve(&crate::workspace::resolve(&graph, &engine.snapshot()));
    engine.remove_device_set(set).expect("close the radio");
    let problems = ProblemBoard::default();
    push(&engine, &problems, &routes, "gps", Some(&fix()));
    assert_eq!(problems.of("planes").len(), 1);
}

#[test]
fn the_radar_transmitter_reads_the_fix_of_its_wired_position() {
    let (_, graph, _, _) = bench();
    let mut snapshot = WorkspaceSnapshot::empty();
    snapshot.graph = graph;
    let store = crate::Store::open(None).expect("store");
    let id = store
        .create_workspace("radar", &snapshot)
        .expect("workspace");
    store.activate_workspace(id).expect("activate");
    let app = crate::AppState::new(Engine::new(None), Arc::new(store));
    app.resolve_now();
    app.gps.reconcile(&app);
    let resolved = app.resolved.current();
    let tx = resolved
        .patch
        .first_feed("radar", sdrmm_wire::RADAR_TX_PORT)
        .expect("the tx input is fed");
    let fix = app.gps.fix(&tx.source).expect("the tx source has a fix");
    assert!((fix.latitude - 52.0).abs() < 1e-9);
    assert!(resolved.positions.reaches("radar"));
}

#[test]
fn a_position_problem_leaves_with_its_wire() {
    let (engine, mut graph, _, _) = bench();
    graph.edges.retain(|edge| edge.to.node != "planes");
    let mut snapshot = WorkspaceSnapshot::empty();
    snapshot.graph = graph;
    let store = crate::Store::open(None).expect("store");
    let id = store
        .create_workspace("unwired", &snapshot)
        .expect("workspace");
    store.activate_workspace(id).expect("activate");
    let app = crate::AppState::new(engine, Arc::new(store));
    for node in ["planes", "rec"] {
        app.problems.set(
            &app.engine,
            sdrmm_wire::ProblemSource::Position,
            node,
            vec!["Busy".to_owned()],
        );
    }
    app.resolve_now();
    app.gps.reconcile(&app);
    assert!(app.problems.of("planes").is_empty());
    assert_eq!(app.problems.of("rec"), ["Busy"]);
}

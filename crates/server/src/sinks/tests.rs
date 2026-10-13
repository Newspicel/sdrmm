use sdrmm_wire::{
    AdsbMessage, ChannelNode, DecoderEvent, EventAudio, EventFilterNode, EventOrigin, FilterMode,
    NodeBody, PatchEdge, PatchGraph, PatchNode, PortRef, Position, StateSnapshot, VoiceCall,
};
use tokio::sync::broadcast::error::TryRecvError;

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

fn channel(id: &str, channel_type: &str) -> PatchNode {
    node(
        id,
        NodeBody::Channel(ChannelNode {
            channel_type: channel_type.to_owned(),
            tuning_locked: false,
        }),
    )
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

fn events(from: &str, to: &str) -> PatchEdge {
    wire((from, "events"), (to, "events"))
}

fn from(source: &str, event: DecoderEvent) -> DecodedRecord {
    DecodedRecord {
        origin: Some(EventOrigin {
            node: source.to_owned(),
            transmission: 0,
        }),
        device_set: 1,
        channel: 1,
        at: "2026-09-21T10:00:00Z".to_owned(),
        freq_hz: 1_090_000_000.0,
        event,
        sinks: Vec::new(),
        view: None,
        callsign: None,
    }
}

fn aircraft(icao: &str, callsign: &str) -> DecoderEvent {
    DecoderEvent::Adsb(AdsbMessage {
        icao: icao.to_owned(),
        df: 17,
        callsign: Some(callsign.to_owned()),
        lat: Some(52.5),
        lon: Some(13.4),
        ..AdsbMessage::default()
    })
}

fn call(node: &str) -> DecoderEvent {
    DecoderEvent::Call(VoiceCall {
        id: 7,
        node: node.to_owned(),
        started_at: "2026-09-21T10:00:00Z".to_owned(),
        ended_at: "2026-09-21T10:00:02Z".to_owned(),
        duration_ms: 2_000,
        device_set: 1,
        channel: 1,
        freq_hz: 439_000_000.0,
        mode: "dmr".to_owned(),
        slot: Some(1),
        color_code: None,
        source: Some(2_620_001),
        destination: Some(9),
        group_call: Some(true),
        encrypted: false,
        emergency: false,
        audio: Some(EventAudio {
            url: "/api/calls/7/audio".to_owned(),
            media_type: "audio/wav".to_owned(),
        }),
        audio_error: None,
    })
}

struct Bench {
    hub: SinkHub,
    resolved: Resolved,
}

impl Bench {
    fn over(graph: PatchGraph) -> Self {
        let resolved = Resolved::of(graph, &StateSnapshot::default());
        let hub = SinkHub::default();
        hub.rewire(&resolved, |_| None);
        Self { hub, resolved }
    }

    fn send(&self, record: DecodedRecord) {
        let mut record = record;
        self.resolved
            .events
            .route(&mut record, &mut crate::events::Stations::default());
        self.hub.deliver(&record, Instant::now());
    }

    fn feed(&self, sink: &str) -> SinkFeed {
        self.hub.feed(sink).expect("the node keeps a sink feed")
    }
}

fn two_decoders_one_map() -> PatchGraph {
    PatchGraph {
        nodes: vec![
            channel("north", "adsb"),
            channel("south", "adsb"),
            node(
                "quiet",
                NodeBody::EventFilter(EventFilterNode {
                    mode: FilterMode::Drop,
                    contains: Some("TEST".to_owned()),
                    ..EventFilterNode::default()
                }),
            ),
            node("map", NodeBody::Map),
            node("readout", NodeBody::Readout),
        ],
        edges: vec![
            events("north", "map"),
            events("south", "quiet"),
            events("quiet", "map"),
            events("north", "readout"),
        ],
    }
}

#[test]
fn a_map_holds_only_entities_its_wires_and_filters_pass() {
    let bench = Bench::over(two_decoders_one_map());

    bench.send(from("north", aircraft("3c6444", "DLH123")));
    bench.send(from("south", aircraft("4ca2d4", "TEST01")));
    bench.send(from("south", aircraft("abc123", "EZY456")));

    let keys: Vec<(String, String)> = bench
        .feed("map")
        .entities
        .into_iter()
        .map(|entity| (entity.key.source, entity.key.id))
        .collect();
    assert_eq!(
        keys,
        [
            ("north".to_owned(), "3C6444".to_owned()),
            ("south".to_owned(), "ABC123".to_owned()),
        ]
    );
    assert_eq!(bench.feed("readout").entities.len(), 1);
    assert_eq!(bench.feed("map").records.len(), 2);
}

#[test]
fn a_session_watching_one_sink_gets_nothing_for_another() {
    let bench = Bench::over(two_decoders_one_map());
    let (_, mut map) = bench.hub.subscribe("map").expect("map is a sink");
    let (_, mut readout) = bench.hub.subscribe("readout").expect("readout is a sink");

    bench.send(from("south", aircraft("abc123", "EZY456")));
    bench.hub.flush(&SurfaceHub::default());

    let update = map.try_recv().expect("the map got its update");
    assert!(update.as_str().contains("\"SinkUpdate\""));
    assert!(update.as_str().contains("ABC123"));
    assert!(matches!(readout.try_recv(), Err(TryRecvError::Empty)));
}

#[test]
fn a_snapshot_carries_what_the_sink_holds() {
    let bench = Bench::over(two_decoders_one_map());
    bench.send(from("north", aircraft("3c6444", "DLH123")));

    let (snapshot, _) = bench.hub.subscribe("map").expect("map is a sink");

    let event: ServerEvent = serde_json::from_str(snapshot.as_str()).expect("snapshot json");
    let ServerEvent::SinkSnapshot { node, feed } = event else {
        panic!("a sink subscription opens with a snapshot");
    };
    assert_eq!(node, "map");
    assert_eq!(feed.entities.len(), 1);
}

#[test]
fn removing_a_wire_drops_what_it_brought() {
    let mut graph = two_decoders_one_map();
    let bench = Bench::over(graph.clone());
    bench.send(from("north", aircraft("3c6444", "DLH123")));
    bench.send(from("south", aircraft("abc123", "EZY456")));

    graph.edges.retain(|edge| edge.from.node != "north");
    let rewired = Resolved::of(graph, &StateSnapshot::default());
    bench.hub.rewire(&rewired, |_| None);

    let sources: Vec<String> = bench
        .feed("map")
        .entities
        .into_iter()
        .map(|entity| entity.key.source)
        .collect();
    assert_eq!(sources, ["south"]);
    assert!(
        bench
            .feed("map")
            .records
            .iter()
            .all(|record| record.origin.as_ref().is_some_and(|o| o.node == "south"))
    );
}

#[test]
fn a_clip_stays_only_in_a_sink_that_keeps_clips() {
    let graph = PatchGraph {
        nodes: vec![
            channel("dmr", "dmr"),
            node("plain", NodeBody::Readout),
            node("heard", NodeBody::DecoderLog(Default::default())),
        ],
        edges: vec![events("dmr", "plain"), events("dmr", "heard")],
    };
    let bench = Bench::over(graph);

    bench.send(from("dmr", call("dmr")));

    let clip = |sink: &str| {
        let record = bench.feed(sink).records.remove(0);
        let DecoderEvent::Call(call) = record.event else {
            panic!("the call reached {sink}");
        };
        (
            call.audio.is_some(),
            record.view.and_then(|view| view.clip).is_some(),
        )
    };
    assert_eq!(clip("plain"), (false, false));
    assert_eq!(clip("heard"), (true, true));
}

fn gps_and_map() -> PatchGraph {
    PatchGraph {
        nodes: vec![
            node("gps", NodeBody::Gps(Default::default())),
            channel("adsb", "adsb"),
            node("map", NodeBody::Map),
        ],
        edges: vec![
            events("adsb", "map"),
            wire(("gps", "position"), ("map", "position")),
        ],
    }
}

fn berlin() -> PositionFix {
    PositionFix {
        latitude: 52.5,
        longitude: 13.4,
        altitude_m: None,
        accuracy_m: None,
        speed_mps: None,
        track_deg: None,
        time: "2026-09-21T10:00:00Z".to_owned(),
        attitude: sdrmm_wire::Attitude::default(),
    }
}

#[test]
fn unwiring_a_gps_sends_an_empty_track_list() {
    let mut graph = gps_and_map();
    let bench = Bench::over(graph.clone());
    bench.hub.position("gps", Some(&berlin()));
    let (_, mut updates) = bench.hub.subscribe("map").expect("map is a sink");
    bench.hub.flush(&SurfaceHub::default());
    let _first = updates.try_recv().expect("the track went out");

    graph.edges.retain(|edge| edge.from.node != "gps");
    bench
        .hub
        .rewire(&Resolved::of(graph, &StateSnapshot::default()), |_| None);
    bench.hub.flush(&SurfaceHub::default());

    let text = updates.try_recv().expect("the removal went out");
    let event: ServerEvent = serde_json::from_str(text.as_str()).expect("update json");
    let ServerEvent::SinkUpdate { update, .. } = event else {
        panic!("a sink update");
    };
    assert_eq!(update.tracks, Some(Vec::new()));
}

#[test]
fn a_gps_wired_into_a_map_becomes_a_track() {
    let bench = Bench::over(gps_and_map());
    let fix = berlin();

    bench.hub.position("gps", Some(&fix));
    bench.hub.position("elsewhere", Some(&fix));

    let tracks = bench.feed("map").tracks;
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].source, "gps");
    assert_eq!(tracks[0].trail.len(), 1);
}

fn satellite_fix() -> DecoderEvent {
    DecoderEvent::Satellite(sdrmm_wire::SatelliteFix {
        name: Some("ISS (ZARYA)".to_owned()),
        catalog: Some("25544".to_owned()),
        ground: Some(sdrmm_wire::SatelliteGround {
            at: sdrmm_wire::LatLon {
                lat: 51.0,
                lon: 10.0,
            },
            altitude_km: 420.0,
            footprint_km: 2_200.0,
        }),
        ground_track: vec![
            sdrmm_wire::LatLon {
                lat: 50.0,
                lon: 9.0,
            },
            sdrmm_wire::LatLon {
                lat: 51.0,
                lon: 10.0,
            },
        ],
        look: None,
        visible: true,
        doppler_hz: None,
        next_pass: None,
    })
}

fn satellite_and_map(wired: bool) -> PatchGraph {
    PatchGraph {
        nodes: vec![
            node("sat", NodeBody::Satellite(Default::default())),
            node("map", NodeBody::Map),
        ],
        edges: if wired {
            vec![events("sat", "map")]
        } else {
            Vec::new()
        },
    }
}

#[test]
fn a_satellite_wired_to_a_map_is_an_entity_with_footprint_and_track() {
    let bench = Bench::over(satellite_and_map(true));

    bench.send(from("sat", satellite_fix()));

    let entities = bench.feed("map").entities;
    assert_eq!(entities.len(), 1);
    let shapes = &entities[0].view.shapes;
    assert!(matches!(
        shapes[0],
        sdrmm_wire::present::Shape::Circle { .. }
    ));
    assert!(matches!(shapes[1], sdrmm_wire::present::Shape::Line { .. }));
    assert_eq!(entities[0].symbol, Some(sdrmm_wire::Symbol::Satellite));
}

#[test]
fn a_lost_satellite_leaves_the_map() {
    let bench = Bench::over(satellite_and_map(true));
    bench.send(from("sat", satellite_fix()));
    assert_eq!(bench.feed("map").entities.len(), 1);

    let lost = sdrmm_wire::SatelliteFix::of(&sdrmm_wire::SatelliteStatus::default());
    bench.send(from("sat", DecoderEvent::Satellite(lost)));

    assert!(bench.feed("map").entities.is_empty());
}

#[test]
fn an_unwired_satellite_reaches_no_map() {
    let bench = Bench::over(satellite_and_map(false));
    let mut record = from("sat", satellite_fix());

    let outcome = bench
        .resolved
        .events
        .route(&mut record, &mut crate::events::Stations::default());
    bench.hub.deliver(&record, Instant::now());

    assert_eq!(outcome, crate::events::RouteOutcome::Unrouted);
    assert!(bench.feed("map").entities.is_empty());
}

#[test]
fn broadcast_status_reaches_a_filtered_readout_only_through_its_wire() {
    let graph = PatchGraph {
        nodes: vec![
            channel("dab", "dab"),
            node(
                "status-only",
                NodeBody::EventFilter(EventFilterNode {
                    kinds: vec![sdrmm_wire::EventKind::Broadcast],
                    ..EventFilterNode::default()
                }),
            ),
            node("wired", NodeBody::Readout),
            node("loose", NodeBody::Readout),
        ],
        edges: vec![events("dab", "status-only"), events("status-only", "wired")],
    };
    let bench = Bench::over(graph);
    let status: DecoderEvent = serde_json::from_value(serde_json::json!({
        "kind": "broadcast",
        "data": {"system": "dab_plus", "locked": true, "snr_db": 18.0, "frequency_error_hz": 0.0}
    }))
    .expect("broadcast status json");
    let slide: DecoderEvent = serde_json::from_value(serde_json::json!({
        "kind": "broadcast_data",
        "data": {"name": "slide.jpg", "media_type": "image/jpeg", "bytes": [1]}
    }))
    .expect("broadcast data json");

    bench.send(from("dab", status));
    bench.send(from("dab", slide));

    let kinds: Vec<sdrmm_wire::EventKind> = bench
        .feed("wired")
        .records
        .iter()
        .map(|record| record.event.kind())
        .collect();
    assert_eq!(kinds, [sdrmm_wire::EventKind::Broadcast]);
    assert!(bench.feed("loose").records.is_empty());
}

#[test]
fn a_propagation_node_turns_wired_spots_into_cells_from_its_receiver() {
    let graph = PatchGraph {
        nodes: vec![
            node("gps", NodeBody::Gps(Default::default())),
            channel("ft8", "ft8"),
            node("prop", NodeBody::Propagation(Default::default())),
        ],
        edges: vec![
            events("ft8", "prop"),
            wire(("gps", "position"), ("prop", "position")),
        ],
    };
    let bench = Bench::over(graph);
    let fix = PositionFix {
        latitude: 52.5,
        longitude: 13.0,
        altitude_m: None,
        accuracy_m: None,
        speed_mps: None,
        track_deg: None,
        time: "2026-09-21T10:00:00Z".to_owned(),
        attitude: sdrmm_wire::Attitude::default(),
    };
    bench.hub.position("gps", Some(&fix));
    let mut record = from(
        "ft8",
        DecoderEvent::Ft8(sdrmm_wire::WsjtMessage {
            text: "CQ W1AW FN42".to_owned(),
            snr_db: -12.0,
            audio_hz: 1_500.0,
            time_offset_s: 0.0,
            hard_errors: 0,
        }),
    );
    record.at = jiff::Timestamp::now().to_string();

    bench.send(record);

    let view = bench
        .feed("prop")
        .propagation
        .expect("a propagation node keeps a model");
    assert!(view.receiver.is_some());
    assert_eq!(view.summary.decodes, 1);
    assert_eq!(view.cells.len(), 2, "two hops, two reflection squares");
    let (_, mut updates) = bench.hub.subscribe("prop").expect("prop is a sink");
    bench.hub.flush(&SurfaceHub::default());
    let update = updates.try_recv().expect("the model went out");
    assert!(update.as_str().contains("\"propagation\""));
}

#[test]
fn clearing_a_sink_drops_what_it_held() {
    let bench = Bench::over(two_decoders_one_map());
    bench.send(from("north", aircraft("3c6444", "DLH123")));

    assert!(bench.hub.clear("map"));

    let feed = bench.feed("map");
    assert!(feed.entities.is_empty() && feed.records.is_empty());
    assert!(!bench.hub.clear("nowhere"));
}

#[test]
fn a_clear_empties_every_watcher_and_drops_what_waited_for_the_flush() {
    let bench = Bench::over(two_decoders_one_map());
    let (_, mut updates) = bench.hub.subscribe("map").expect("map is a sink");
    bench.send(from("north", aircraft("3c6444", "DLH123")));

    bench.hub.clear("map");
    bench.hub.flush(&SurfaceHub::default());

    let sent = updates.try_recv().expect("the clear went out");
    let event: ServerEvent = serde_json::from_str(sent.as_str()).expect("snapshot json");
    let ServerEvent::SinkSnapshot { feed, .. } = event else {
        panic!("a clear sends a fresh snapshot");
    };
    assert!(feed.entities.is_empty() && feed.records.is_empty());
    assert!(
        matches!(updates.try_recv(), Err(TryRecvError::Empty)),
        "nothing from before the clear follows it"
    );
}

#[test]
fn records_lost_upstream_count_on_every_event_sink() {
    let bench = Bench::over(two_decoders_one_map());
    let mut counted = bench.hub.lost(3);
    counted.sort();
    assert_eq!(counted, ["map", "readout"]);
    assert_eq!(bench.feed("map").lost, 3);
}

#[test]
fn a_record_its_station_let_through_survives_a_rewire() {
    let graph = PatchGraph {
        nodes: vec![
            channel("adsb", "adsb"),
            node(
                "placed",
                NodeBody::EventFilter(EventFilterNode {
                    has_position: Some(true),
                    ..EventFilterNode::default()
                }),
            ),
            node("readout", NodeBody::Readout),
        ],
        edges: vec![events("adsb", "placed"), events("placed", "readout")],
    };
    let bench = Bench::over(graph);
    let mut stations = crate::events::Stations::default();
    let ident = DecoderEvent::Adsb(AdsbMessage {
        icao: "3c6444".to_owned(),
        df: 17,
        callsign: Some("DLH123".to_owned()),
        ..AdsbMessage::default()
    });
    for event in [aircraft("3c6444", "DLH123"), ident] {
        let mut record = from("adsb", event);
        bench.resolved.events.route(&mut record, &mut stations);
        bench.hub.deliver(&record, Instant::now());
    }
    assert_eq!(bench.feed("readout").records.len(), 2);

    bench.hub.rewire(&bench.resolved, |_| None);

    assert_eq!(bench.feed("readout").records.len(), 2);
}

fn picture(sinks: &[&str]) -> CapturedImage {
    CapturedImage {
        id: 3,
        node: "sstv".to_owned(),
        sinks: sinks.iter().map(|sink| (*sink).to_owned()).collect(),
        device_set: 1,
        channel: 1,
        at: "2026-09-21T10:00:00Z".to_owned(),
        freq_hz: 14_230_000.0,
        source: "sstv".to_owned(),
        mode: "Martin M1".to_owned(),
        width: 320,
        height: 256,
        lines: 256,
        complete: true,
        image: None,
        image_error: None,
    }
}

#[test]
fn a_video_node_shows_the_pictures_its_wire_brought() {
    let bench = Bench::over(PatchGraph {
        nodes: vec![
            channel("sstv", "sstv"),
            node("screen", NodeBody::Video),
            node("spare", NodeBody::Video),
        ],
        edges: vec![wire(("sstv", "video"), ("screen", "video"))],
    });
    bench.hub.picture(&picture(&["screen"]));
    assert_eq!(bench.feed("screen").pictures.len(), 1);
    assert!(bench.feed("spare").pictures.is_empty());

    let late = Bench::over(PatchGraph {
        nodes: vec![node("screen", NodeBody::Video)],
        edges: Vec::new(),
    });
    late.hub.seed_pictures(|sink| {
        if sink == "screen" {
            vec![picture(&["screen"])]
        } else {
            Vec::new()
        }
    });
    assert_eq!(
        late.feed("screen").pictures.len(),
        1,
        "a late session gets the gallery"
    );
}

#[test]
fn a_picture_the_store_let_go_leaves_the_gallery() {
    let bench = Bench::over(PatchGraph {
        nodes: vec![channel("sstv", "sstv"), node("screen", NodeBody::Video)],
        edges: vec![wire(("sstv", "video"), ("screen", "video"))],
    });
    bench.hub.picture(&picture(&["screen"]));
    let (_, mut updates) = bench.hub.subscribe("screen").expect("video is a sink");
    bench.hub.flush(&SurfaceHub::default());
    let _shown = updates.try_recv().expect("the picture went out");

    bench.hub.reload_pictures(|_| Vec::new());
    bench.hub.flush(&SurfaceHub::default());

    assert!(bench.feed("screen").pictures.is_empty());
    let text = updates.try_recv().expect("the removal went out");
    let event: ServerEvent = serde_json::from_str(text.as_str()).expect("update json");
    let ServerEvent::SinkUpdate { update, .. } = event else {
        panic!("a sink update");
    };
    assert_eq!(update.pictures, Some(Vec::new()));
}

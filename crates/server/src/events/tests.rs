use sdrmm_wire::{
    AdsbMessage, ChannelNode, DecoderEvent, EventKind, EventOrigin, FilterMode, PatchEdge,
    PatchNode, PortRef, Position, RttyText,
};

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

fn filter(id: &str, settings: EventFilterNode) -> PatchNode {
    node(id, NodeBody::EventFilter(settings))
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

fn edge(from: &str, to: &str) -> PatchEdge {
    wire((from, "events"), (to, "events"))
}

fn only(kind: EventKind) -> EventFilterNode {
    EventFilterNode {
        kinds: vec![kind],
        ..EventFilterNode::default()
    }
}

fn rtty(text: &str) -> DecoderEvent {
    DecoderEvent::Rtty(RttyText {
        text: text.to_owned(),
    })
}

fn aircraft() -> DecoderEvent {
    DecoderEvent::Adsb(AdsbMessage {
        icao: "3c6444".to_owned(),
        ..AdsbMessage::default()
    })
}

fn from(source: Option<&str>, event: DecoderEvent) -> DecodedRecord {
    DecodedRecord {
        origin: source.map(|node| EventOrigin {
            node: node.to_owned(),
            transmission: 0,
        }),
        device_set: 1,
        channel: 2,
        at: "2026-09-21T10:00:00Z".to_owned(),
        freq_hz: 14_080_000.0,
        event,
        sinks: Vec::new(),
        view: None,
        callsign: None,
    }
}

fn routes(graph: &PatchGraph) -> EventRoutes {
    let descriptors = sdrmm_channels::descriptor_table();
    let ctx = sdrmm_wire::ResolveCtx {
        descriptors: Some(descriptors),
        live: None,
    };
    let mut patch =
        sdrmm_wire::patch::resolve::compile(graph, &ctx).expect("the test graph compiles");
    patch.workspace = Some(7);
    EventRoutes::resolve(&patch, graph)
}

fn routed(routes: &EventRoutes, record: DecodedRecord) -> (RouteOutcome, Vec<String>) {
    let mut record = record;
    let outcome = routes.route(&mut record);
    let mut sinks = record.sinks;
    sinks.sort();
    (outcome, sinks)
}

fn rtty_graph() -> PatchGraph {
    PatchGraph {
        nodes: vec![
            channel("rtty", "rtty"),
            filter("only-calls", only(EventKind::Call)),
            node("chat", NodeBody::DecoderLog(Default::default())),
            node("export", NodeBody::Export),
            node("idle", NodeBody::DecoderLog(Default::default())),
        ],
        edges: vec![
            edge("rtty", "only-calls"),
            edge("only-calls", "chat"),
            edge("rtty", "export"),
        ],
    }
}

#[test]
fn a_routed_record_names_the_sinks_it_reached_after_every_filter() {
    let routes = routes(&rtty_graph());

    let mut record = from(Some("rtty"), rtty("CQ"));
    assert_eq!(routes.route(&mut record), RouteOutcome::Delivered);

    assert_eq!(record.sinks, ["export"]);
    assert_eq!(routes.workspace, Some(7));
    assert_eq!(
        record.view.map(|view| view.summary).as_deref(),
        Some("CQ"),
        "the router stamps the presentation once"
    );
}

#[test]
fn a_drop_filter_on_the_wire_removes_what_it_names() {
    let graph = PatchGraph {
        nodes: vec![
            channel("rtty", "rtty"),
            filter(
                "no-cq",
                EventFilterNode {
                    mode: FilterMode::Drop,
                    contains: Some("cq".to_owned()),
                    ..EventFilterNode::default()
                },
            ),
            node("chat", NodeBody::DecoderLog(Default::default())),
        ],
        edges: vec![edge("rtty", "no-cq"), edge("no-cq", "chat")],
    };
    let routes = routes(&graph);

    assert_eq!(
        routed(&routes, from(Some("rtty"), rtty("CQ"))),
        (RouteOutcome::Unrouted, Vec::new())
    );
    assert_eq!(routed(&routes, from(Some("rtty"), rtty("73"))).1, ["chat"]);
}

#[test]
fn a_kind_the_sink_does_not_accept_is_not_stamped_and_counts_as_ignored() {
    let graph = PatchGraph {
        nodes: vec![channel("dmr", "dmr"), node("map", NodeBody::Map)],
        edges: vec![edge("dmr", "map")],
    };
    let routes = routes(&graph);
    let call = DecoderEvent::Call(sdrmm_wire::VoiceCall {
        id: 1,
        node: "dmr".to_owned(),
        started_at: "2026-09-21T10:00:00Z".to_owned(),
        ended_at: "2026-09-21T10:00:02Z".to_owned(),
        duration_ms: 2_000,
        device_set: 1,
        channel: 2,
        freq_hz: 439_000_000.0,
        mode: "dmr".to_owned(),
        slot: None,
        color_code: None,
        source: Some(2_620_001),
        destination: Some(9),
        group_call: Some(true),
        encrypted: false,
        emergency: false,
        audio: None,
        audio_error: None,
    });

    assert_eq!(
        routed(&routes, from(Some("dmr"), call)),
        (RouteOutcome::Ignored, Vec::new())
    );
}

#[test]
fn a_record_no_wire_takes_is_unrouted() {
    let routes = routes(&rtty_graph());

    assert_eq!(
        routed(&routes, from(Some("elsewhere"), rtty("CQ"))),
        (RouteOutcome::Unrouted, Vec::new())
    );
}

#[test]
fn a_record_without_origin_is_unattributed_and_reaches_nothing() {
    let routes = routes(&rtty_graph());
    let mut record = from(None, rtty("CQ"));
    record.sinks = vec!["export".to_owned()];

    assert_eq!(routes.route(&mut record), RouteOutcome::Unattributed);
    assert!(record.sinks.is_empty());
}

#[test]
fn only_decoders_wired_into_adsb_feed_truth() {
    let graph = PatchGraph {
        nodes: vec![
            channel("wired", "adsb"),
            channel("filtered", "adsb"),
            channel("logged", "adsb"),
            filter("quiet", EventFilterNode::default()),
            node(
                "radar",
                NodeBody::PassiveRadar(sdrmm_wire::PassiveRadarNode::default()),
            ),
            node("chat", NodeBody::DecoderLog(Default::default())),
        ],
        edges: vec![
            wire(("wired", "events"), ("radar", sdrmm_wire::RADAR_TRUTH_PORT)),
            edge("filtered", "quiet"),
            wire(("quiet", "events"), ("radar", sdrmm_wire::RADAR_TRUTH_PORT)),
            edge("logged", "chat"),
        ],
    };
    let routes = routes(&graph);

    assert_eq!(
        routed(&routes, from(Some("wired"), aircraft())).1,
        ["radar"]
    );
    assert_eq!(
        routed(&routes, from(Some("filtered"), aircraft())).1,
        ["radar"],
        "a filter between the decoder and adsb still counts"
    );
    assert_eq!(
        routed(&routes, from(Some("logged"), aircraft())).1,
        ["chat"],
        "a decoder wired elsewhere feeds no truth"
    );
}

#[test]
fn one_open_wire_is_enough_to_reach_a_sink_once() {
    let graph = PatchGraph {
        nodes: vec![
            channel("rtty", "rtty"),
            filter("only-calls", only(EventKind::Call)),
            node("chat", NodeBody::DecoderLog(Default::default())),
        ],
        edges: vec![
            edge("rtty", "only-calls"),
            edge("only-calls", "chat"),
            edge("rtty", "chat"),
        ],
    };
    let routes = routes(&graph);

    assert_eq!(routed(&routes, from(Some("rtty"), rtty("CQ"))).1, ["chat"]);
}

#[test]
fn a_filter_is_never_a_sink() {
    let routes = routes(&rtty_graph());

    assert!(routes.reaches("rtty", "chat"));
    assert!(routes.reaches("rtty", "export"));
    assert!(!routes.reaches("rtty", "only-calls"));
    assert!(!routes.reaches("rtty", "idle"));
}

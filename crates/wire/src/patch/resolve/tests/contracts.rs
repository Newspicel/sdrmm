use super::*;
use crate::{
    EventOutputNode, EventOutputTarget, KindSet, WebhookFormat,
    patch::{DmrTrunkNode, Keeps, ScannerNode, SignalGenNode},
};

fn decoders() -> Vec<ChannelDescriptor> {
    let decoder = |ty: &str, emits: &[EventKind], calls: Option<CallGate>| ChannelDescriptor {
        type_id: ty.to_owned(),
        emits: emits.to_vec(),
        calls,
        ..ChannelDescriptor::default()
    };
    vec![
        decoder(
            "nfm",
            &[EventKind::Tone, EventKind::Scrambler],
            Some(CallGate::Squelch),
        ),
        decoder("dmr", &[EventKind::Dv], Some(CallGate::Frames)),
        decoder("adsb", &[EventKind::Adsb], None),
        decoder("rtty", &[EventKind::Rtty], None),
        ChannelDescriptor {
            type_id: "atv".to_owned(),
            has_audio: false,
            has_video: true,
            ..ChannelDescriptor::default()
        },
    ]
}

fn draft(graph: &PatchGraph) -> (ResolvedPatch, Vec<RefusedEdge>) {
    let all = decoders();
    compile_draft(
        graph,
        &ResolveCtx {
            descriptors: Some(&all),
            live: None,
        },
    )
}

fn reasons(graph: &PatchGraph) -> Vec<(PatchEdge, String)> {
    draft(graph)
        .1
        .into_iter()
        .map(|refused| (refused.edge, refused.reason))
        .collect()
}

fn kinds(list: &[EventKind]) -> KindSet {
    list.iter().copied().collect()
}

fn map(id: &str) -> PatchNode {
    node(id, NodeBody::Map)
}

fn output(id: &str, target: EventOutputTarget) -> PatchNode {
    node(id, NodeBody::EventOutput(EventOutputNode { target }))
}

#[test]
fn dv_reaches_a_map_and_the_map_ignores_its_calls() {
    let graph = PatchGraph {
        nodes: vec![channel("dmr", "dmr"), map("map")],
        edges: vec![events("dmr", "map")],
    };
    let (patch, refused) = draft(&graph);
    assert!(refused.is_empty());
    let input = patch
        .node("map")
        .and_then(|found| found.input("events"))
        .expect("events input");
    assert_eq!(input.ignored, kinds(&[EventKind::Call]));
    assert_eq!(
        input.feeds[0].payload.kinds(),
        kinds(&[EventKind::Dv, EventKind::Call])
    );
    assert_eq!(
        patch.reaches("dmr", "events")[0].kinds,
        kinds(&[EventKind::Dv])
    );
}

#[test]
fn nfm_into_a_map_is_refused() {
    let graph = PatchGraph {
        nodes: vec![channel("nfm", "nfm"), map("map")],
        edges: vec![events("nfm", "map")],
    };
    assert_eq!(
        reasons(&graph),
        [(events("nfm", "map"), "Map takes positions".to_owned())]
    );
    assert!(
        draft(&graph).0.edges.is_empty(),
        "a refused wire feeds nothing"
    );
}

#[test]
fn a_satellite_reaches_a_map_but_not_a_decoder_log() {
    let graph = PatchGraph {
        nodes: vec![
            node("sat", NodeBody::Satellite(Default::default())),
            map("map"),
            node("log", NodeBody::DecoderLog(Default::default())),
        ],
        edges: vec![events("sat", "map"), events("sat", "log")],
    };
    let refused = reasons(&graph);
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].0, events("sat", "log"));
    assert_eq!(
        draft(&graph).0.reaches("sat", "events")[0].kinds,
        kinds(&[EventKind::Satellite])
    );
}

#[test]
fn a_radio_and_a_generator_share_a_decoder_but_a_beam_stands_alone() {
    let shared = PatchGraph {
        nodes: vec![
            radio("dev", "band"),
            node("gen", NodeBody::SignalGen(SignalGenNode::default())),
            channel("voice", "nfm"),
        ],
        edges: vec![
            wire(("dev", "iq"), ("voice", "iq")),
            wire(("gen", "iq"), ("voice", "iq")),
        ],
    };
    assert!(reasons(&shared).is_empty());

    let mut beamed = beam_graph();
    beamed.edges.push(wire(("dev", "iq2"), ("voice", "iq")));
    assert_eq!(
        reasons(&beamed),
        [(
            wire(("dev", "iq2"), ("voice", "iq")),
            "Only radio lanes share an input".to_owned()
        )]
    );
}

#[test]
fn a_filter_wired_first_is_refused_on_the_wire_that_breaks_it() {
    let graph = PatchGraph {
        nodes: vec![
            node(
                "tri",
                NodeBody::Triangulation(crate::patch::TriangulationNode::default()),
            ),
            filter("only", &[]),
            channel("text", "rtty"),
        ],
        edges: vec![events("only", "tri"), events("text", "only")],
    };
    let (patch, refused) = draft(&graph);
    assert_eq!(
        refused,
        [RefusedEdge {
            edge: events("text", "only"),
            reason: "Triangulation takes bearings".to_owned(),
        }]
    );
    assert_eq!(patch.edges.len(), 1);
    assert!(patch.feeds("tri", "events").is_empty());
}

#[test]
fn a_filter_carries_only_the_kinds_it_lets_through() {
    let graph = PatchGraph {
        nodes: vec![
            channel("planes", "adsb"),
            channel("voice", "dmr"),
            filter("only", &[EventKind::Adsb]),
            log("log"),
        ],
        edges: vec![
            events("planes", "only"),
            events("voice", "only"),
            events("only", "log"),
        ],
    };
    let (patch, refused) = draft(&graph);
    assert!(refused.is_empty());
    let out = patch
        .node("only")
        .and_then(|found| found.output("events"))
        .expect("filter output");
    assert_eq!(out.spec.payload.kinds(), kinds(&[EventKind::Adsb]));
    let carried: Vec<(&str, KindSet)> = patch
        .feeds("log", "events")
        .iter()
        .map(|feed| (feed.source.as_str(), feed.payload.kinds()))
        .collect();
    assert_eq!(
        carried,
        [
            ("planes", kinds(&[EventKind::Adsb])),
            ("voice", KindSet::EMPTY)
        ]
    );
    assert!(patch.reaches("voice", "events")[0].kinds.is_empty());
}

#[test]
fn an_event_output_takes_what_its_target_sends() {
    let beast = EventOutputTarget::Beast {
        address: "127.0.0.1:30005".to_owned(),
        enabled: true,
    };
    let webhook = EventOutputTarget::Webhook {
        url: "https://example.com/hook".to_owned(),
        format: WebhookFormat::Json,
    };
    let graph = |target: EventOutputTarget, decoder: &str| PatchGraph {
        nodes: vec![channel("src", decoder), output("out", target)],
        edges: vec![events("src", "out")],
    };
    assert!(reasons(&graph(beast.clone(), "adsb")).is_empty());
    assert_eq!(
        reasons(&graph(beast, "nfm")),
        [(events("src", "out"), "Event output takes ADS-B".to_owned())]
    );
    assert!(reasons(&graph(webhook, "nfm")).is_empty());
    assert!(reasons(&graph(EventOutputTarget::Recordings, "dmr")).is_empty());
    assert_eq!(
        reasons(&graph(EventOutputTarget::Recordings, "adsb")),
        [(
            events("src", "out"),
            "Event output takes calls and transmissions".to_owned()
        )]
    );
}

#[test]
fn only_an_event_output_that_sends_audio_keeps_clips() {
    let keeps = |target: EventOutputTarget| {
        NodeBody::EventOutput(crate::EventOutputNode { target })
            .ports(None)
            .iter()
            .any(|port| port.keeps.contains(&Keeps::Clips))
    };
    let webhook = |format| EventOutputTarget::Webhook {
        url: "https://example.com/hook".to_owned(),
        format,
    };
    assert!(keeps(EventOutputTarget::Recordings));
    assert!(keeps(webhook(WebhookFormat::Discord)));
    assert!(!keeps(webhook(WebhookFormat::Json)));
    assert!(!keeps(EventOutputTarget::Desktop));
    let graph = PatchGraph {
        nodes: vec![
            channel("voice", "dmr"),
            output("out", EventOutputTarget::Recordings),
        ],
        edges: vec![events("voice", "out"), audio("voice", "out")],
    };
    assert_eq!(
        reasons(&graph)
            .iter()
            .map(|(edge, _)| edge)
            .collect::<Vec<_>>(),
        [&audio("voice", "out")],
        "clips ride on the events wire"
    );
}

#[test]
fn compile_returns_the_first_refusal_and_draft_lists_every_one() {
    let graph = PatchGraph {
        nodes: vec![channel("nfm", "nfm"), channel("text", "rtty"), map("map")],
        edges: vec![events("nfm", "map"), events("text", "map")],
    };
    assert_eq!(reasons(&graph).len(), 2);
    let all = decoders();
    let error = compile(
        &graph,
        &ResolveCtx {
            descriptors: Some(&all),
            live: None,
        },
    )
    .expect_err("refused");
    assert_eq!(error.to_string(), "Map takes positions");
}

fn kept(patch: &ResolvedPatch, node: &str, port: &str) -> Vec<Keeps> {
    patch
        .node(node)
        .and_then(|found| found.output(port))
        .map(|output| output.kept.clone())
        .unwrap_or_default()
}

fn only(kinds: &[EventKind]) -> NodeBody {
    NodeBody::EventFilter(crate::filter::EventFilterNode {
        kinds: kinds.to_vec(),
        ..Default::default()
    })
}

#[test]
fn outputs_know_what_their_sinks_keep() {
    let graph = PatchGraph {
        nodes: vec![
            channel("voice", "dmr"),
            channel("pager", "adsb"),
            channel("tv", "atv"),
            log("log"),
            node("vid", NodeBody::Video),
            output(
                "out",
                EventOutputTarget::Webhook {
                    url: "https://example.com/hook".to_owned(),
                    format: WebhookFormat::Discord,
                },
            ),
        ],
        edges: vec![
            events("voice", "log"),
            events("voice", "out"),
            events("pager", "log"),
            wire(("tv", "video"), ("vid", "video")),
        ],
    };
    let (patch, refused) = draft(&graph);
    assert!(refused.is_empty());
    assert_eq!(
        kept(&patch, "voice", "events"),
        [Keeps::History, Keeps::Clips]
    );
    assert_eq!(kept(&patch, "pager", "events"), [Keeps::History]);
    assert_eq!(kept(&patch, "tv", "video"), [Keeps::Pictures]);
    assert!(patch.takes_kept("log", "events", Keeps::Clips));
    assert!(!patch.takes_kept("log", "events", Keeps::Pictures));
}

#[test]
fn a_filter_that_drops_calls_keeps_no_clips() {
    let graph = PatchGraph {
        nodes: vec![
            channel("voice", "dmr"),
            node("keep", only(&[EventKind::Dv])),
            log("log"),
        ],
        edges: vec![events("voice", "keep"), events("keep", "log")],
    };
    let (patch, refused) = draft(&graph);
    assert!(refused.is_empty());
    assert_eq!(kept(&patch, "voice", "events"), [Keeps::History]);
}

#[test]
fn trunks_hand_out_audio_monitors_hand_out_clips_and_scanners_watch_a_lane() {
    let graph = PatchGraph {
        nodes: vec![
            radio("dev", "band"),
            node("trunk", NodeBody::DmrTrunk(DmrTrunkNode::default())),
            node("monitor", NodeBody::SpectrumMonitor(Default::default())),
            node("scan", NodeBody::Scanner(ScannerNode::default())),
            node("spk", NodeBody::Speaker),
            log("log"),
        ],
        edges: vec![
            wire(("dev", "iq"), ("scan", "iq")),
            audio("trunk", "spk"),
            events("monitor", "log"),
            audio("monitor", "spk"),
        ],
    };
    assert_eq!(
        reasons(&graph)
            .iter()
            .map(|(edge, _)| edge)
            .collect::<Vec<_>>(),
        [&audio("monitor", "spk")],
        "a monitor has no audio output"
    );
    assert!(draft(&graph).0.keeps("monitor", "events", Keeps::Clips));
    assert_eq!(
        draft(&graph).0.feeds("scan", "iq")[0].source,
        "dev",
        "a scanner names the lane it watches"
    );
}

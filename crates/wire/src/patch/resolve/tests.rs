use super::{feeds::MAX_PASS_DEPTH, *};
use crate::{
    CallGate, EventKind,
    array::VirtualLane,
    channel::{ChannelInfo, ChannelSettings},
    filter::EventFilterNode,
    patch::{
        AudioRecorderNode, BeamformerNode, ChannelNode, DeviceNode, DeviceRef, PatchEdge,
        PatchNode, Position,
    },
    state::{DeviceSet, TrunkControl, TrunkFollower, TrunkSystemStatus},
};

fn node(id: &str, body: NodeBody) -> PatchNode {
    PatchNode {
        id: id.to_owned(),
        body,
        position: Position { x: 0.0, y: 0.0 },
        size: None,
        label: None,
    }
}

fn channel(id: &str, ty: &str) -> PatchNode {
    node(
        id,
        NodeBody::Channel(ChannelNode {
            channel_type: ty.to_owned(),
            tuning_locked: false,
        }),
    )
}

fn radio(id: &str, key: &str) -> PatchNode {
    node(
        id,
        NodeBody::Device(DeviceNode {
            device: Some(DeviceRef {
                backend: "virtual".to_owned(),
                serial: None,
                key: Some(key.to_owned()),
            }),
            ..DeviceNode::default()
        }),
    )
}

fn filter(id: &str, kinds: &[EventKind]) -> PatchNode {
    node(
        id,
        NodeBody::EventFilter(EventFilterNode {
            kinds: kinds.to_vec(),
            ..EventFilterNode::default()
        }),
    )
}

fn fx(id: &str) -> PatchNode {
    node(id, NodeBody::AudioFx(crate::AudioFxNode::default()))
}

fn log(id: &str) -> PatchNode {
    node(id, NodeBody::DecoderLog(Default::default()))
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

fn audio(from: &str, to: &str) -> PatchEdge {
    wire((from, "audio"), (to, "audio"))
}

fn descriptors() -> Vec<ChannelDescriptor> {
    ["nfm", "dmr", "adsb"]
        .into_iter()
        .map(|ty| ChannelDescriptor {
            type_id: ty.to_owned(),
            name: ty.to_owned(),
            bandwidth_hz: 12_500.0,
            input_rate_hz: 48_000.0,
            emits: vec![EventKind::Dv],
            calls: Some(CallGate::Frames),
            ..ChannelDescriptor::default()
        })
        .collect()
}

fn compiled(graph: &PatchGraph) -> ResolvedPatch {
    compile(graph, &ResolveCtx::default()).expect("compiles")
}

fn bound(graph: &PatchGraph, live: &StateSnapshot) -> (ResolvedPatch, Vec<NodeProblem>) {
    bound_with(graph, live, &[])
}

fn bound_with(
    graph: &PatchGraph,
    live: &StateSnapshot,
    attached: &[crate::device::DeviceInfo],
) -> (ResolvedPatch, Vec<NodeProblem>) {
    let all = descriptors();
    let ctx = ResolveCtx {
        descriptors: Some(&all),
        live: Some(live),
    };
    let mut patch = compile(graph, &ctx).expect("compiles");
    let problems = bind(&mut patch, graph, live, attached);
    (patch, problems)
}

fn sources(feeds: &[Feed]) -> Vec<(&str, Vec<&str>)> {
    let mut found: Vec<(&str, Vec<&str>)> = feeds
        .iter()
        .map(|feed| {
            (
                feed.source.as_str(),
                feed.via.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    found.sort();
    found
}

fn set(id: u32, key: &str, channels: Vec<ChannelInfo>) -> DeviceSet {
    let mut set = crate::contract_tests::sample_device_set();
    set.id = id;
    set.device.key = key.to_owned();
    set.channels = channels;
    set
}

fn live_channel(id: u32, node: Option<&str>, ty: &str, stream: u32) -> ChannelInfo {
    ChannelInfo {
        id,
        stream,
        node: node.map(str::to_owned),
        settings: ChannelSettings::default_for(ty).expect("channel type"),
        out_of_band: None,
        rejected: 0,
        skipped: 0,
        audio_recordings: Vec::new(),
        baseband_recording: None,
        network_export: None,
    }
}

fn live(sets: Vec<DeviceSet>) -> StateSnapshot {
    StateSnapshot {
        device_sets: sets,
        ..StateSnapshot::default()
    }
}

#[test]
fn a_direct_wire_feeds_the_sink_with_nothing_in_between() {
    let graph = PatchGraph {
        nodes: vec![channel("dmr", "dmr"), log("chat")],
        edges: vec![events("dmr", "chat")],
    };
    let patch = compiled(&graph);
    assert_eq!(
        sources(patch.feeds("chat", "events")),
        [("dmr", Vec::new())]
    );
    let reaches = patch.reaches("dmr", "events");
    assert_eq!(reaches.len(), 1);
    assert_eq!(reaches[0].sink, "chat");
}

#[test]
fn a_chain_of_filters_names_the_decoder_behind_it_with_every_filter_in_order() {
    let graph = PatchGraph {
        nodes: vec![
            channel("dmr", "dmr"),
            filter("kind", &[EventKind::Call]),
            filter("group", &[]),
            log("chat"),
        ],
        edges: vec![
            events("dmr", "kind"),
            events("kind", "group"),
            events("group", "chat"),
        ],
    };
    let patch = compiled(&graph);
    assert_eq!(
        sources(patch.feeds("chat", "events")),
        [("dmr", vec!["kind", "group"])]
    );
    assert_eq!(patch.reaches("dmr", "events")[0].via, ["kind", "group"]);
    assert_eq!(patch.reaches("dmr", "events")[0].sink, "chat");
    assert_eq!(
        sources(patch.feeds("group", "events")),
        [("dmr", vec!["kind"])]
    );
}

#[test]
fn one_filter_serves_several_decoders_and_a_decoder_can_arrive_twice() {
    let graph = PatchGraph {
        nodes: vec![
            channel("dmr", "dmr"),
            channel("p25", "dmr"),
            filter("calls", &[EventKind::Call]),
            log("chat"),
        ],
        edges: vec![
            events("dmr", "calls"),
            events("p25", "calls"),
            events("calls", "chat"),
            events("dmr", "chat"),
        ],
    };
    let patch = compiled(&graph);
    assert_eq!(
        sources(patch.feeds("chat", "events")),
        [
            ("dmr", Vec::new()),
            ("dmr", vec!["calls"]),
            ("p25", vec!["calls"])
        ]
    );
}

#[test]
fn a_filter_with_nothing_downstream_reaches_nothing() {
    let graph = PatchGraph {
        nodes: vec![channel("dmr", "dmr"), filter("calls", &[]), log("chat")],
        edges: vec![events("dmr", "calls")],
    };
    let patch = compiled(&graph);
    assert!(patch.reaches("dmr", "events").is_empty());
    assert!(patch.feeds("chat", "events").is_empty());
}

#[test]
fn audio_runs_through_an_fx_chain_channel_side_first() {
    let graph = PatchGraph {
        nodes: vec![
            channel("a", "nfm"),
            channel("b", "nfm"),
            fx("near"),
            fx("far"),
            node("rec", NodeBody::AudioRecorder(AudioRecorderNode::default())),
        ],
        edges: vec![
            audio("a", "near"),
            audio("near", "far"),
            audio("far", "rec"),
            audio("b", "rec"),
        ],
    };
    let patch = compiled(&graph);
    assert_eq!(
        sources(patch.feeds("rec", "audio")),
        [("a", vec!["near", "far"]), ("b", Vec::new())]
    );
}

#[test]
fn an_fx_chain_deeper_than_the_cap_feeds_nothing() {
    let depth = MAX_PASS_DEPTH + 1;
    let mut nodes = vec![channel("a", "nfm"), node("speaker", NodeBody::Speaker)];
    let mut edges = vec![audio("a", "fx0")];
    for index in 0..depth {
        nodes.push(fx(&format!("fx{index}")));
        if index > 0 {
            edges.push(audio(&format!("fx{}", index - 1), &format!("fx{index}")));
        }
    }
    edges.push(audio(&format!("fx{}", depth - 1), "speaker"));
    let graph = PatchGraph { nodes, edges };
    let patch = compiled(&graph);
    assert!(patch.feeds("speaker", "audio").is_empty());
    let shallow = sources(patch.feeds(&format!("fx{}", MAX_PASS_DEPTH - 1), "audio"));
    assert_eq!(shallow.len(), 1);
}

#[test]
fn a_radio_binds_to_the_first_open_set_it_names_and_claims_it_once() {
    let graph = PatchGraph {
        nodes: vec![
            radio("one", "band"),
            radio("two", "band"),
            radio("far", "away"),
        ],
        edges: Vec::new(),
    };
    let state = live(vec![set(4, "band", Vec::new())]);
    let (patch, problems) = bound(&graph, &state);
    assert_eq!(patch.device_set("one"), Some(4));
    assert_eq!(patch.device_set("two"), None);
    assert_eq!(
        patch.runs("far"),
        Some(&Runs::Unbound {
            reason: UnboundReason::RadioAbsent
        })
    );
    assert!(
        problems
            .iter()
            .any(|problem| problem.node == "far"
                && problem.text == UnboundReason::RadioAbsent.text())
    );
}

#[test]
fn a_decoder_binds_only_the_channel_opened_for_it_on_a_wired_lane() {
    let graph = PatchGraph {
        nodes: vec![radio("dev", "band"), channel("voice", "dmr")],
        edges: vec![wire(("dev", "iq"), ("voice", "iq"))],
    };
    let state = live(vec![set(
        1,
        "band",
        vec![
            live_channel(3, None, "dmr", 0),
            live_channel(4, Some("trunk"), "dmr", 0),
            live_channel(5, Some("voice"), "nfm", 0),
            live_channel(6, Some("voice"), "dmr", 0),
        ],
    )]);
    let (patch, problems) = bound(&graph, &state);
    assert_eq!(
        patch.runs("voice"),
        Some(&Runs::Channel {
            device_set: 1,
            channel: 6,
            stream: 0
        })
    );
    assert!(problems.is_empty());
    assert_eq!(
        patch.feeds("voice", "iq")[0].tap,
        Some(Tap::Lane {
            device_set: 1,
            stream: 0
        })
    );
}

#[test]
fn a_decoder_on_another_lane_or_unwired_stays_unbound() {
    let graph = PatchGraph {
        nodes: vec![
            radio("dev", "band"),
            channel("voice", "dmr"),
            channel("loose", "dmr"),
        ],
        edges: vec![wire(("dev", "iq2"), ("voice", "iq"))],
    };
    let state = live(vec![set(
        1,
        "band",
        vec![
            live_channel(5, Some("voice"), "dmr", 0),
            live_channel(6, Some("loose"), "dmr", 0),
        ],
    )]);
    let (patch, problems) = bound(&graph, &state);
    assert_eq!(patch.channel("voice"), None);
    assert_eq!(
        patch.runs("loose"),
        Some(&Runs::Unbound {
            reason: UnboundReason::NotWired
        })
    );
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].text, UnboundReason::NotPlaced.text());
}

#[test]
fn a_decoder_runs_on_whichever_wired_radio_carries_it() {
    let graph = PatchGraph {
        nodes: vec![
            radio("a", "one"),
            radio("b", "two"),
            channel("voice", "dmr"),
        ],
        edges: vec![
            wire(("a", "iq"), ("voice", "iq")),
            wire(("b", "iq"), ("voice", "iq")),
        ],
    };
    let state = live(vec![
        set(1, "one", Vec::new()),
        set(2, "two", vec![live_channel(9, Some("voice"), "dmr", 0)]),
    ]);
    let (patch, _) = bound(&graph, &state);
    assert_eq!(
        patch.channel("voice"),
        Some(ChannelAddr {
            device_set: 2,
            channel: 9
        })
    );
    assert_eq!(
        patch.owner_of(ChannelAddr {
            device_set: 2,
            channel: 9
        }),
        Some("voice")
    );
}

#[test]
fn a_trunk_system_runs_on_its_control_and_follower_channels() {
    let graph = PatchGraph {
        nodes: vec![
            node("trunk", NodeBody::DmrTrunk(Default::default())),
            node("speaker", NodeBody::Speaker),
        ],
        edges: Vec::new(),
    };
    let mut state = live(Vec::new());
    state.trunk_systems.push(TrunkSystemStatus {
        node: "trunk".to_owned(),
        detected: None,
        carriers: 1,
        control: Some(TrunkControl {
            device_set: 1,
            channel: 2,
            freq_hz: 1,
        }),
        followers: vec![TrunkFollower {
            device_set: 1,
            channel: 3,
            logical_channel: None,
            slot: 1,
            freq_hz: 2,
        }],
        problems: Vec::new(),
        channel_map: Vec::new(),
        probes: Vec::new(),
        searching: 0,
        candidates: 0,
        other_control_hz: Vec::new(),
        color_code: None,
    });
    let (patch, _) = bound(&graph, &state);
    let control = ChannelAddr {
        device_set: 1,
        channel: 2,
    };
    let follower = ChannelAddr {
        device_set: 1,
        channel: 3,
    };
    assert_eq!(
        patch.runs("trunk"),
        Some(&Runs::Trunk {
            control: Some(control),
            followers: vec![follower]
        })
    );
    assert_eq!(patch.owner_of(follower), Some("trunk"));
}

#[test]
fn a_feed_from_a_bound_decoder_taps_its_channel() {
    let graph = PatchGraph {
        nodes: vec![
            radio("dev", "band"),
            channel("voice", "nfm"),
            fx("clean"),
            node("speaker", NodeBody::Speaker),
        ],
        edges: vec![
            wire(("dev", "iq"), ("voice", "iq")),
            audio("voice", "clean"),
            audio("clean", "speaker"),
        ],
    };
    let state = live(vec![set(
        1,
        "band",
        vec![live_channel(7, Some("voice"), "nfm", 0)],
    )]);
    let (patch, _) = bound(&graph, &state);
    let feed = &patch.feeds("speaker", "audio")[0];
    assert_eq!(feed.via, ["clean"]);
    assert_eq!(
        feed.tap,
        Some(Tap::Channel {
            device_set: 1,
            channel: 7
        })
    );
}

fn beam_graph() -> PatchGraph {
    PatchGraph {
        nodes: vec![
            radio("dev", "band"),
            node("arr", NodeBody::Array(crate::array::ArrayNode::default())),
            node("bf", NodeBody::Beamformer(BeamformerNode::default())),
            channel("voice", "nfm"),
        ],
        edges: vec![
            wire(("dev", "iq"), ("arr", "lane")),
            wire(("arr", "array"), ("bf", "array")),
            wire(("bf", "beam"), ("voice", "iq")),
        ],
    }
}

#[test]
fn a_decoder_on_a_beam_binds_on_the_virtual_lane_the_engine_opened() {
    let graph = beam_graph();
    let mut radio_set = set(1, "band", vec![live_channel(8, Some("voice"), "nfm", 5)]);
    radio_set.virtual_lanes.push(VirtualLane {
        stream: 5,
        node: "bf".to_owned(),
        port: "beam".to_owned(),
        center_hz: 1.0,
        sample_rate: 1.0,
    });
    let (patch, _) = bound(&graph, &live(vec![radio_set]));
    assert_eq!(
        patch.feeds("voice", "iq")[0].tap,
        Some(Tap::Lane {
            device_set: 1,
            stream: 5
        })
    );
    assert_eq!(
        patch.runs("voice"),
        Some(&Runs::Channel {
            device_set: 1,
            channel: 8,
            stream: 5
        })
    );
    assert_eq!(patch.runs("bf"), Some(&Runs::Engine));
    assert_eq!(patch.runs("arr"), Some(&Runs::Engine));
}

#[test]
fn no_virtual_lane_listed_means_no_tap_yet() {
    let graph = beam_graph();
    let (patch, _) = bound(&graph, &live(vec![set(1, "band", Vec::new())]));
    assert_eq!(patch.feeds("voice", "iq")[0].tap, None);
}

#[test]
fn repeated_ports_follow_the_radio_and_keep_saved_wires() {
    let graph = PatchGraph {
        nodes: vec![radio("dev", "band"), channel("voice", "nfm")],
        edges: vec![wire(("dev", "iq3"), ("voice", "iq"))],
    };
    let names = |patch: &ResolvedPatch| -> Vec<String> {
        patch
            .node("dev")
            .map(|node| node.outputs().map(|port| port.spec.name.clone()).collect())
            .unwrap_or_default()
    };
    assert_eq!(names(&compiled(&graph)), ["iq", "iq2", "iq3"]);
    let mut wide = set(1, "band", Vec::new());
    wide.capabilities.rx_streams = 4;
    let (patch, _) = bound(&graph, &live(vec![wide]));
    assert_eq!(names(&patch), ["iq", "iq2", "iq3", "iq4"]);
}

#[test]
fn a_channel_shows_only_the_ports_its_descriptor_offers() {
    let graph = PatchGraph {
        nodes: vec![channel("voice", "adsb")],
        edges: Vec::new(),
    };
    let mut all = descriptors();
    all[2].has_audio = false;
    let ctx = ResolveCtx {
        descriptors: Some(&all),
        live: None,
    };
    let patch = compile(&graph, &ctx).expect("compiles");
    let names: Vec<&str> = patch
        .node("voice")
        .map(|node| {
            node.ports
                .iter()
                .map(|port| port.spec.name.as_str())
                .collect()
        })
        .unwrap_or_default();
    assert!(!names.contains(&"audio"));
    assert!(names.contains(&"events"));
}

#[test]
fn a_decoder_retunes_its_radio_lane_unless_either_end_is_locked() {
    let mut graph = PatchGraph {
        nodes: vec![radio("dev", "band"), channel("voice", "nfm")],
        edges: vec![wire(("dev", "iq"), ("voice", "iq"))],
    };
    let mut manual = set(1, "band", Vec::new());
    manual.settings.tuning = Some(crate::Tuning::Manual);
    for state in [live(vec![set(1, "band", Vec::new())]), live(vec![manual])] {
        assert!(bound(&graph, &state).0.edges[0].retunes_source);
    }
    let state = live(vec![set(1, "band", Vec::new())]);
    if let NodeBody::Channel(channel) = &mut graph.nodes[1].body {
        channel.tuning_locked = true;
    }
    assert!(!bound(&graph, &state).0.edges[0].retunes_source);
    if let NodeBody::Channel(channel) = &mut graph.nodes[1].body {
        channel.tuning_locked = false;
    }
    if let NodeBody::Device(device) = &mut graph.nodes[0].body {
        device.locked_streams.push(0);
    }
    assert!(!bound(&graph, &state).0.edges[0].retunes_source);
}

#[test]
fn a_decoder_on_an_array_lane_retunes_the_array() {
    let graph = beam_graph();
    let mut radio_set = set(1, "band", Vec::new());
    radio_set.virtual_lanes.push(VirtualLane {
        stream: 5,
        node: "bf".to_owned(),
        port: "beam".to_owned(),
        center_hz: 1.0,
        sample_rate: 1.0,
    });
    let (patch, _) = bound(&graph, &live(vec![radio_set]));
    let retunes = |to: &str| {
        patch
            .edges
            .iter()
            .find(|edge| edge.to.node == to)
            .is_some_and(|edge| edge.retunes_source)
    };
    assert!(retunes("voice"));
    assert!(!retunes("arr"));
    assert!(!retunes("bf"));
}

#[test]
fn compile_refuses_what_validate_refuses() {
    let graph = PatchGraph {
        nodes: vec![log("a"), log("b")],
        edges: vec![events("a", "b")],
    };
    assert!(matches!(
        compile(&graph, &ResolveCtx::default()),
        Err(PatchError::Direction(_))
    ));
}

#[test]
fn the_resolved_patch_serializes_its_tags() {
    let graph = PatchGraph {
        nodes: vec![radio("dev", "band"), channel("voice", "nfm")],
        edges: vec![wire(("dev", "iq"), ("voice", "iq"))],
    };
    let state = live(vec![set(
        1,
        "band",
        vec![live_channel(8, Some("voice"), "nfm", 0)],
    )]);
    let (patch, _) = bound(&graph, &state);
    let json = serde_json::to_value(&patch).expect("json");
    let voice = &json["nodes"][1];
    assert_eq!(voice["runs"]["runs"], "channel");
    assert_eq!(voice["ports"][0]["name"], "iq");
    assert_eq!(voice["ports"][0]["feeds"][0]["tap"]["tap"], "lane");
    let back: ResolvedPatch = serde_json::from_value(json).expect("round trip");
    assert_eq!(back, patch);
}

#[test]
fn an_unbound_decoder_says_why_from_its_radio() {
    let graph = |reference: Option<&str>| PatchGraph {
        nodes: vec![
            match reference {
                Some(key) => radio("dev", key),
                None => node("dev", NodeBody::Device(DeviceNode::default())),
            },
            channel("voice", "nfm"),
            channel("loose", "nfm"),
        ],
        edges: vec![wire(("dev", "iq"), ("voice", "iq"))],
    };
    let reason = |patch: &ResolvedPatch, node: &str| match patch.runs(node) {
        Some(Runs::Unbound { reason }) => Some(*reason),
        _ => None,
    };
    let nothing = live(Vec::new());
    let (patch, _) = bound(&graph(None), &nothing);
    assert_eq!(reason(&patch, "dev"), Some(UnboundReason::NoRadio));
    assert_eq!(reason(&patch, "voice"), Some(UnboundReason::NoRadio));
    assert_eq!(reason(&patch, "loose"), Some(UnboundReason::NotWired));

    let (patch, _) = bound(&graph(Some("band")), &nothing);
    assert_eq!(reason(&patch, "voice"), Some(UnboundReason::RadioAbsent));

    let mut plugged = set(1, "band", Vec::new()).device;
    plugged.key = "band".to_owned();
    let (patch, problems) = bound_with(&graph(Some("band")), &nothing, &[plugged]);
    assert_eq!(reason(&patch, "dev"), Some(UnboundReason::RadioClosed));
    assert_eq!(reason(&patch, "voice"), Some(UnboundReason::RadioClosed));
    assert!(
        problems
            .iter()
            .any(|problem| problem.text == UnboundReason::RadioClosed.text())
    );

    let (patch, _) = bound(
        &graph(Some("band")),
        &live(vec![set(1, "band", Vec::new())]),
    );
    assert_eq!(reason(&patch, "voice"), Some(UnboundReason::NotPlaced));

    let (patch, _) = bound(&beam_graph(), &live(vec![set(1, "band", Vec::new())]));
    assert_eq!(reason(&patch, "voice"), Some(UnboundReason::NotRunning));
}

fn stopped(id: &str, key: &str) -> PatchNode {
    let mut held = radio(id, key);
    if let NodeBody::Device(device) = &mut held.body {
        device.stopped = true;
    }
    held
}

#[test]
fn a_stopped_radio_frees_its_device_and_says_so() {
    let graph = PatchGraph {
        nodes: vec![stopped("dev", "band"), channel("voice", "nfm")],
        edges: vec![wire(("dev", "iq"), ("voice", "iq"))],
    };
    let state = live(vec![set(1, "band", Vec::new())]);
    let (patch, problems) = bound(&graph, &state);
    let reason = |node: &str| match patch.runs(node) {
        Some(Runs::Unbound { reason }) => Some(*reason),
        _ => None,
    };
    assert_eq!(reason("dev"), Some(UnboundReason::Stopped));
    assert_eq!(reason("voice"), Some(UnboundReason::Stopped));
    assert!(
        graph
            .node("dev")
            .and_then(|node| node.body.device_ref("dev"))
            .is_none()
    );
    assert!(
        problems
            .iter()
            .all(|problem| problem.text != UnboundReason::RadioClosed.text())
    );
}

#[test]
fn a_stopped_array_member_leaves_the_beam_unbound() {
    let mut graph = beam_graph();
    graph.nodes[0] = stopped("dev", "band");
    let (patch, _) = bound(&graph, &live(vec![set(1, "band", Vec::new())]));
    assert_eq!(
        patch.runs("dev"),
        Some(&Runs::Unbound {
            reason: UnboundReason::Stopped
        })
    );
    assert!(patch.feeds("arr", "lane")[0].tap.is_none());
    assert!(matches!(patch.runs("voice"), Some(Runs::Unbound { .. })));
}

#[test]
fn a_controller_sees_where_its_target_runs_and_a_target_who_sweeps_it() {
    let graph = PatchGraph {
        nodes: vec![
            radio("dev", "band"),
            channel("voice", "nfm"),
            node("scan", NodeBody::Scanner(Default::default())),
        ],
        edges: vec![
            wire(("dev", "iq"), ("voice", "iq")),
            wire(("dev", "iq"), ("scan", "iq")),
            wire(("scan", "control"), ("voice", "control")),
        ],
    };
    let mut radio_set = set(1, "band", vec![live_channel(8, Some("voice"), "nfm", 0)]);
    let (patch, _) = bound(&graph, &live(vec![radio_set.clone()]));
    let target = &patch.reaches("scan", "control")[0];
    assert_eq!(
        target.target_runs,
        Some(Runs::Channel {
            device_set: 1,
            channel: 8,
            stream: 0
        })
    );
    assert_eq!(patch.feeds("voice", "control")[0].drives, None);

    radio_set.scanners.push(crate::scan::ScannerStatus {
        settings: crate::scan::ScanSettings {
            channel: 8,
            node: Some("scan".to_owned()),
            ..Default::default()
        },
        ..serde_json::from_value(serde_json::json!({
            "state": "scanning", "settings": {"channel": 8, "ranges": []},
            "targets": 0, "current_hz": 0.0, "sweeps": 0, "hits": 0
        }))
        .expect("a scanner status")
    });
    let (patch, _) = bound(&graph, &live(vec![radio_set]));
    assert_eq!(
        patch.feeds("voice", "control")[0].drives,
        Some(crate::control::ControlVerb::Sweep)
    );
}

#[test]
fn a_decoder_is_held_by_its_lock_or_by_the_node_on_its_control_input() {
    let mut graph = PatchGraph {
        nodes: vec![
            channel("voice", "nfm"),
            channel("free", "am"),
            node("scan", NodeBody::Scanner(Default::default())),
        ],
        edges: vec![wire(("scan", "control"), ("voice", "control"))],
    };
    let patch = compiled(&graph);
    let held = |patch: &ResolvedPatch, id: &str| patch.node(id).and_then(|node| node.held.clone());
    assert_eq!(
        held(&patch, "voice"),
        Some(Held::Wire {
            node: "scan".to_owned()
        })
    );
    assert_eq!(held(&patch, "free"), None);
    assert_eq!(held(&patch, "scan"), None);
    assert_eq!(patch.may_tune("free", None), Ok(()));
    assert_eq!(patch.may_tune("voice", Some("scan")), Ok(()));
    assert_eq!(
        patch.may_tune("voice", None),
        Err("Tuned by Scanner".to_owned())
    );

    if let NodeBody::Channel(locked) = &mut graph.nodes[0].body {
        locked.tuning_locked = true;
    }
    let patch = compiled(&graph);
    assert_eq!(held(&patch, "voice"), Some(Held::Hand));
    assert_eq!(
        patch.may_tune("voice", Some("scan")),
        Err("Tuning locked".to_owned())
    );
}

#[test]
fn a_coherent_radio_offers_the_wires_of_a_new_array() {
    let graph = PatchGraph {
        nodes: vec![radio("dev", "kraken")],
        edges: Vec::new(),
    };
    let mut kraken = set(1, "kraken", Vec::new());
    kraken.capabilities.rx_streams = 3;
    kraken.capabilities.coherence = crate::device::Coherence::PhaseCoherent;
    let offer = |state: &StateSnapshot, graph: &PatchGraph| {
        bound(graph, state)
            .0
            .node("dev")
            .and_then(|node| node.array_offer.clone())
    };
    let wire_of = |from: &str, to: &str| LaneWire {
        from: from.to_owned(),
        to: to.to_owned(),
    };
    assert_eq!(
        offer(&live(vec![kraken.clone()]), &graph),
        Some(ArrayOffer::Ready {
            wires: vec![
                wire_of("iq", "lane"),
                wire_of("iq2", "lane2"),
                wire_of("iq3", "lane3"),
            ]
        })
    );

    let mut loose = kraken.clone();
    loose.capabilities.coherence = crate::device::Coherence::None;
    assert_eq!(
        offer(&live(vec![loose]), &graph),
        Some(ArrayOffer::Refused {
            reason: crate::Refusal::SharedClock.text()
        })
    );

    let held = PatchGraph {
        nodes: vec![
            radio("dev", "kraken"),
            node("arr", NodeBody::Array(crate::array::ArrayNode::default())),
        ],
        edges: vec![wire(("dev", "iq"), ("arr", "lane"))],
    };
    assert_eq!(offer(&live(vec![kraken]), &held), None);
    assert_eq!(
        offer(&live(vec![set(1, "kraken", Vec::new())]), &graph),
        None
    );
}

mod contracts;
mod verdicts;

#[test]
fn wires_past_the_cap_are_refused_and_the_rest_still_run() {
    let graph = PatchGraph {
        nodes: vec![channel("dmr", "dmr"), log("chat")],
        edges: vec![events("dmr", "chat"); crate::patch::MAX_EDGES + 1],
    };
    let (patch, refused) = compile_draft(&graph, &ResolveCtx::default());
    assert_eq!(
        sources(patch.feeds("chat", "events")),
        [("dmr", Vec::new())]
    );
    assert_eq!(refused.len(), crate::patch::MAX_EDGES);
    assert_eq!(
        refused.last().map(|edge| edge.reason.clone()),
        Some(PatchError::TooManyEdges.to_string())
    );
}

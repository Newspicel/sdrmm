use sdrmm_wire::{
    ChannelNode, DeviceBackend, DeviceNode, DeviceRef, PatchEdge, PatchNode, PortRef, Position,
    SpeakerNode, SpeakerOutput,
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

fn wire(from: (&str, &str), to: (&str, &str)) -> PatchEdge {
    let port = |(node, port): (&str, &str)| PortRef {
        node: node.to_owned(),
        port: port.to_owned(),
    };
    PatchEdge {
        from: port(from),
        to: port(to),
    }
}

fn speaker(output: Option<SpeakerOutput>) -> NodeBody {
    NodeBody::Speaker(SpeakerNode {
        output,
        ..SpeakerNode::default()
    })
}

fn on_server(device: Option<&str>) -> NodeBody {
    speaker(Some(SpeakerOutput::Server {
        server: device.map(str::to_owned),
    }))
}

fn radio(dir: &tempfile::TempDir) -> (Arc<Engine>, u32, u32) {
    let mut registry = sdrmm_device::DeviceRegistry::new();
    registry.register(1, Box::new(sdrmm_device_virtual::VirtualDriver::new()));
    let engine = Engine::with_registry(registry, Some(dir.path().to_path_buf()));
    let ds = engine.create_device_set("virtual:band").expect("radio");
    let settings = sdrmm_decoder::default_settings("nfm").expect("nfm");
    let ch = engine
        .add_channel_for(ds, 0, settings, Some("ch"))
        .expect("channel");
    (engine, ds, ch)
}

fn patch(speakers: Vec<(&str, NodeBody)>) -> PatchGraph {
    let mut graph = PatchGraph {
        nodes: vec![
            node(
                "dev",
                NodeBody::Device(DeviceNode {
                    device: Some(DeviceRef {
                        backend: DeviceBackend::VIRTUAL,
                        serial: None,
                        key: None,
                    }),
                    ..DeviceNode::default()
                }),
            ),
            node(
                "ch",
                NodeBody::Channel(ChannelNode {
                    channel_type: "nfm".to_owned(),
                    tuning_locked: false,
                    audio: Default::default(),
                }),
            ),
        ],
        edges: vec![wire(("dev", "iq"), ("ch", "iq"))],
    };
    for (id, body) in speakers {
        graph.nodes.push(node(id, body));
        graph.edges.push(wire(("ch", "audio"), (id, "audio")));
    }
    graph
}

fn resolved(graph: PatchGraph, engine: &Engine) -> Resolved {
    Resolved {
        patch: crate::workspace::resolve(&graph, &engine.snapshot()),
        graph: Arc::new(graph),
        ..Resolved::default()
    }
}

#[test]
fn only_speakers_on_the_server_are_played_here() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let (engine, ds, ch) = radio(&dir);
    let current = resolved(
        patch(vec![
            ("local", speaker(None)),
            (
                "named",
                speaker(Some(SpeakerOutput::Listener("js8".to_owned()))),
            ),
            ("default", on_server(None)),
            ("card", on_server(Some("USB"))),
        ]),
        &engine,
    );
    let wanted = wanted(&current.graph, &current.patch);
    let routes = HashMap::from([(AudioRoute::channel(ds, ch), 1.0)]);
    assert_eq!(
        wanted,
        HashMap::from([
            (
                "default".to_owned(),
                Wanted {
                    device: None,
                    routes: routes.clone(),
                },
            ),
            (
                "card".to_owned(),
                Wanted {
                    device: Some("USB".to_owned()),
                    routes,
                },
            ),
        ])
    );
    engine.remove_device_set(ds).expect("close");
}

#[test]
fn a_wire_plays_at_its_fader() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let (engine, ds, ch) = radio(&dir);
    let mut quiet = SpeakerNode {
        output: Some(SpeakerOutput::Server { server: None }),
        ..SpeakerNode::default()
    };
    quiet.volumes.insert("ch".to_owned(), 0.0);
    let current = resolved(patch(vec![("quiet", NodeBody::Speaker(quiet))]), &engine);
    let wanted = wanted(&current.graph, &current.patch);
    assert_eq!(wanted["quiet"].routes[&AudioRoute::channel(ds, ch)], 0.0);
    engine.remove_device_set(ds).expect("close");
}

#[test]
fn an_unwired_speaker_opens_nothing() {
    let engine = Engine::new(None);
    let mut graph = patch(Vec::new());
    graph.nodes.push(node("idle", on_server(None)));
    let current = resolved(graph, &engine);
    assert!(wanted(&current.graph, &current.patch).is_empty());
}

#[tokio::test]
async fn a_card_that_is_not_there_shows_on_its_speaker() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let (engine, ds, _) = radio(&dir);
    let current = resolved(
        patch(vec![("card", on_server(Some("no such card")))]),
        &engine,
    );
    let mut speakers = Speakers::default();
    let problems = speakers.settle(&engine, &current, &|| {}, &Handle::current());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].node, "card");
    assert_eq!(problems[0].source, ProblemSource::Speaker);
    assert!(speakers.playing.is_empty());
    engine.remove_device_set(ds).expect("close");
}

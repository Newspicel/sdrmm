use std::time::{Duration, Instant};

use sdrmm_decoder::VideoPicture;
use sdrmm_wire::{
    ChannelNode, DeviceBackend, DeviceNode, DeviceRef, PatchEdge, PatchNode, PortRef, Position,
    StreamTransport,
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

fn radio(dir: &tempfile::TempDir) -> (Arc<Engine>, u32, u32) {
    let mut registry = sdrmm_device::DeviceRegistry::new();
    registry.register(1, Box::new(sdrmm_device_virtual::VirtualDriver::new()));
    let engine = Engine::with_registry(registry, Some(dir.path().to_path_buf()));
    let ds = engine.create_device_set("virtual:band").expect("radio");
    let settings = sdrmm_decoder::default_settings("atv").expect("atv");
    let ch = engine
        .add_channel_for(ds, 0, settings, Some("ch"))
        .expect("channel");
    (engine, ds, ch)
}

fn patch(wires: &[&str]) -> PatchGraph {
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
                    channel_type: "atv".to_owned(),
                    ..ChannelNode::default()
                }),
            ),
            node("out", NodeBody::StreamOut(StreamOutNode::default())),
        ],
        edges: vec![wire(("dev", "iq"), ("ch", "iq"))],
    };
    for port in wires {
        graph.edges.push(wire(("ch", port), ("out", port)));
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
fn a_stream_follows_its_channel_picture_and_sound() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let (engine, ds, ch) = radio(&dir);
    let current = resolved(patch(&[VIDEO_PORT, AUDIO_PORT]), &engine);
    let wanted = wanted(&current.graph, &current.patch);
    let at = ChannelAddr {
        device_set: ds,
        channel: ch,
    };
    assert_eq!(
        wanted,
        HashMap::from([(
            "out".to_owned(),
            Wanted {
                settings: StreamOutNode::default(),
                video: at,
                audio: Some(AudioRoute::channel(ds, ch)),
            },
        )])
    );
    engine.remove_device_set(ds).expect("close");
}

#[test]
fn a_stream_without_a_picture_wire_runs_nothing() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let (engine, ds, _) = radio(&dir);
    let current = resolved(patch(&[AUDIO_PORT]), &engine);
    assert!(wanted(&current.graph, &current.patch).is_empty());
    engine.remove_device_set(ds).expect("close");
}

fn wanted_http() -> Wanted {
    Wanted {
        settings: StreamOutNode::default(),
        video: ChannelAddr {
            device_set: 1,
            channel: 1,
        },
        audio: None,
    }
}

fn grey(shade: u8) -> Arc<VideoPicture> {
    Arc::new(VideoPicture::grey(64, 48, &[shade; 64 * 48]))
}

#[tokio::test]
async fn a_viewer_receives_transport_packets_of_the_pictures() {
    let hub = StreamHub::default();
    hub.lock().insert(
        "out".to_owned(),
        Running::start("out".to_owned(), wanted_http()),
    );
    let mut viewer = hub.watch("out").expect("served");
    let input = hub.lock()["out"].input.clone();
    let mut received = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    for shade in 0..=u8::MAX {
        input.send(Input::Picture(grey(shade))).expect("queued");
        tokio::time::sleep(Duration::from_millis(5)).await;
        while let Ok(chunk) = viewer.try_recv() {
            received.extend_from_slice(&chunk);
        }
        if received.len() > 10 * 188 || Instant::now() > deadline {
            break;
        }
    }
    assert!(received.len() > 10 * 188, "{} bytes", received.len());
    assert_eq!(received.len() % 188, 0);
    assert!(received.chunks(188).all(|packet| packet[0] == 0x47));
    let status = hub.lock()["out"].stream.status(&StreamOutNode::default());
    assert_eq!(status.clients, 1);
    assert_eq!((status.width, status.height), (64, 48));
    assert_eq!(status.path.as_deref(), Some("/api/streams/out"));
    assert_eq!(status.error, None);
}

#[test]
fn a_sent_stream_serves_no_viewers() {
    let hub = StreamHub::default();
    let mut sent = wanted_http();
    sent.settings.transport = StreamTransport::Udp;
    hub.lock()
        .insert("out".to_owned(), Running::start("out".to_owned(), sent));
    assert!(hub.watch("out").is_none());
    assert!(hub.watch("other").is_none());
}

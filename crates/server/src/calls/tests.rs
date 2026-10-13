use sdrmm_device::DeviceRegistry;
use sdrmm_wire::{DecodedRecord, DecoderEvent, DvFrameKind, DvMode};

use super::*;

fn new_call(encrypted: bool) -> NewCall {
    NewCall {
        node: "calls".to_owned(),
        started_at: "2026-08-14T10:00:00Z".to_owned(),
        ended_at: "2026-08-14T10:00:01Z".to_owned(),
        duration_ms: 1_000,
        device_set: 1,
        channel: 2,
        freq_hz: 451_125_000.0,
        mode: "dmr".to_owned(),
        frame: Some(DvFrame {
            encrypted: Some(encrypted),
            ..DvFrame::new(DvMode::Dmr, DvFrameKind::Header)
        }),
        audio_error: None,
    }
}

#[tokio::test(start_paused = true)]
async fn a_binding_that_arrives_without_a_state_event_still_records() {
    let mut registry = DeviceRegistry::new();
    registry.register(1, Box::new(sdrmm_device_virtual::VirtualDriver::new()));
    let engine = Arc::new(Engine::with_registry(registry, None));
    let set = engine
        .create_device_set("virtual:band")
        .expect("open the virtual radio");
    let channel = engine
        .add_channel_for(
            set,
            0,
            sdrmm_wire::ChannelSettings::default_for("dmr").expect("dmr is built in"),
            Some("radio"),
        )
        .expect("add channel");
    let calls = Arc::new(Calls::default());
    let (resolved, watched) = watch::channel(Arc::new(Resolved::default()));
    let task = tokio::spawn(run(CallWatch {
        engine: Arc::downgrade(&engine),
        calls: calls.clone(),
        resolved: watched,
        problems: Arc::new(ProblemBoard::default()),
    }));
    tokio::time::sleep(Duration::from_millis(10)).await;

    resolved.send_if_modified(|current| {
        *current = Arc::new(calling(set, channel));
        false
    });
    tokio::time::sleep(RECONCILE_INTERVAL * 2).await;
    for kind in [
        DvFrameKind::Header,
        DvFrameKind::Voice,
        DvFrameKind::Terminator,
    ] {
        engine.publish_decoded(DecodedRecord {
            origin: Some(sdrmm_wire::EventOrigin {
                node: "radio".to_owned(),
                transmission: 0,
            }),
            sinks: Vec::new(),
            callsign: None,
            device_set: set,
            channel,
            at: "2026-08-14T10:00:00Z".to_owned(),
            freq_hz: 451_125_000.0,
            event: DecoderEvent::Dv(DvFrame {
                slot: Some(1),
                destination: Some(91),
                ..DvFrame::new(DvMode::Dmr, kind)
            }),
            view: None,
        });
    }

    let listed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let listed = calls.list();
            if !listed.is_empty() {
                return listed;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("a binding that no state event announced is still picked up");
    task.abort();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].node, "radio");
    assert_eq!(listed[0].mode, "dmr");
}

fn calling(device_set: u32, channel: u32) -> Resolved {
    let at = |id: &str, body: sdrmm_wire::NodeBody| sdrmm_wire::PatchNode {
        id: id.to_owned(),
        body,
        position: sdrmm_wire::Position { x: 0.0, y: 0.0 },
        size: None,
        label: None,
    };
    let graph = sdrmm_wire::PatchGraph {
        nodes: vec![
            at(
                "radio",
                sdrmm_wire::NodeBody::Channel(sdrmm_wire::ChannelNode {
                    channel_type: "dmr".to_owned(),
                    tuning_locked: false,
                }),
            ),
            at("log", sdrmm_wire::NodeBody::DecoderLog(Default::default())),
        ],
        edges: vec![sdrmm_wire::PatchEdge {
            from: sdrmm_wire::PortRef {
                node: "radio".to_owned(),
                port: "events".to_owned(),
            },
            to: sdrmm_wire::PortRef {
                node: "log".to_owned(),
                port: "events".to_owned(),
            },
        }],
    };
    let descriptors = sdrmm_engine::channel_types();
    let ctx = sdrmm_wire::ResolveCtx {
        descriptors: Some(&descriptors),
        live: None,
    };
    let mut patch = sdrmm_wire::patch::resolve::compile(&graph, &ctx).expect("compiles");
    for node in &mut patch.nodes {
        if node.id == "radio" {
            node.runs = sdrmm_wire::Runs::Channel {
                device_set,
                channel,
                stream: 0,
            };
        }
    }
    Resolved {
        patch,
        graph: Arc::new(graph),
        ..Resolved::default()
    }
}

#[test]
fn wav_is_mono_8k_pcm() {
    let bytes = wav(&[0, i16::MAX, i16::MIN]);
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 8_000);
    assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 6);
}

#[test]
fn encrypted_completion_has_no_audio() {
    let calls = Calls::default();
    let (stored, _) = calls.push(new_call(true), None, Duration::from_secs(30));
    assert!(stored.encrypted);
    assert!(stored.audio.is_none());
    assert!(calls.audio(stored.id).is_none());
    assert_eq!(calls.list().len(), 1);
}

#[test]
fn a_call_without_frames_has_no_digital_identity() {
    let calls = Calls::default();
    let (stored, _) = calls.push(
        NewCall {
            mode: "am".to_owned(),
            frame: None,
            ..new_call(false)
        },
        None,
        Duration::from_secs(30),
    );
    assert_eq!(stored.mode, "am");
    assert_eq!(stored.mode_label(), "AM");
    assert_eq!(
        (stored.source, stored.destination, stored.slot),
        (None, None, None)
    );
    assert!(!stored.encrypted && !stored.emergency);
}

#[test]
fn evicting_audio_keeps_the_call_and_says_why() {
    let calls = Calls::default();
    let big = Bytes::from(vec![0u8; MAX_STORED_AUDIO_BYTES / 2 + 1]);
    let mut evictions = 0;
    for _ in 0..3 {
        let (_, evicted) = calls.push(new_call(false), Some(big.clone()), Duration::from_secs(30));
        evictions += usize::from(evicted);
    }
    assert!(evictions >= 1, "eviction was never reported");
    let listed = calls.list();
    assert_eq!(listed.len(), 3);
    let evicted = listed.iter().filter(|call| call.audio.is_none()).count();
    assert!(evicted >= 1, "nothing was evicted over the byte limit");
    assert!(
        listed
            .iter()
            .filter(|call| call.audio.is_none())
            .all(|call| call.audio_error.is_some())
    );
    let inner = calls.lock();
    assert!(inner.audio_bytes <= MAX_STORED_AUDIO_BYTES);
}

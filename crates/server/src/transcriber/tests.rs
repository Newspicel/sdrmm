use std::{sync::Arc, time::Duration};

use sdrmm_wire::{
    DecoderEvent, EventContract, NO_CHANNEL, NodeKind, PatchEdge, PatchNode, PortRef, Position,
    VoiceCall, WorkspaceSnapshot,
};

use super::*;
use crate::{ServerOptions, Store, router_with_state, tests::state_over};

const GIVE_UP: Duration = Duration::from_secs(30);

fn call() -> VoiceCall {
    serde_json::from_value(serde_json::json!({
        "id": 7, "node": "trunk", "started_at": "2026-11-23T05:00:00Z",
        "ended_at": "2026-11-23T05:00:04Z", "duration_ms": 4000, "device_set": 0, "channel": 0,
        "freq_hz": 446_000_000.0, "mode": "dmr", "destination": 262, "group_call": true,
        "encrypted": false, "emergency": false,
    }))
    .unwrap()
}

fn record(from: &str) -> DecodedRecord {
    DecodedRecord {
        origin: Some(EventOrigin {
            node: from.to_owned(),
            transmission: 3,
        }),
        device_set: 0,
        channel: NO_CHANNEL,
        at: "2026-11-23T05:00:04Z".to_owned(),
        freq_hz: 446_000_000.0,
        event: DecoderEvent::of(&call()),
        sinks: vec!["tx".to_owned()],
        callsign: None,
        view: None,
    }
}

#[test]
fn a_transcript_rides_on_the_call_and_shows_as_its_text() {
    let mut record = record("trunk");
    with_transcript(&mut record, "Fahrzeug 3 am Einsatzort".to_owned());
    let call = record.event.read::<VoiceCall>().unwrap();
    assert_eq!(call.transcript.as_deref(), Some("Fahrzeug 3 am Einsatzort"));
    let view = record.event.present();
    assert_eq!(view.text.as_deref(), Some("Fahrzeug 3 am Einsatzort"));
    assert!(view.fields.iter().any(|field| field.key == "transcript"));
}

#[test]
fn nothing_heard_leaves_the_call_without_text() {
    let mut record = record("trunk");
    with_transcript(&mut record, String::new());
    assert_eq!(record.event.read::<VoiceCall>().unwrap().transcript, None);
}

#[test]
fn a_passed_event_comes_from_the_transcriber() {
    let passed = passed_on("tx", record("trunk"));
    let origin = passed.origin.unwrap();
    assert_eq!((origin.node.as_str(), origin.transmission), ("tx", 3));
    assert!(passed.sinks.is_empty());
    assert!(passed.view.is_none());
}

#[test]
fn a_stored_clip_reads_back_at_its_rate() {
    let mut wav = vec![0u8; WAV_HEADER];
    wav[24..28].copy_from_slice(&8_000u32.to_le_bytes());
    for sample in [0i16, 16_384, -32_768] {
        wav.extend(sample.to_le_bytes());
    }
    let (rate, pcm) = wav_pcm(&wav).unwrap();
    assert_eq!(rate, 8_000);
    assert_eq!(pcm, [0.0, 0.5, -1.0]);
    assert!(wav_pcm(&wav[..20]).is_err());
}

fn node(id: &str, kind: &str) -> PatchNode {
    PatchNode {
        id: id.to_owned(),
        body: NodeBody::default_for(NodeKind::parse(kind).expect("a known kind")),
        position: Position { x: 0.0, y: 0.0 },
        size: None,
        label: None,
    }
}

fn events(from: &str, to: &str) -> PatchEdge {
    let port = |node: &str| PortRef {
        node: node.to_owned(),
        port: "events".to_owned(),
    };
    PatchEdge {
        from: port(from),
        to: port(to),
    }
}

fn bench() -> AppState {
    let store = Arc::new(Store::open(None).expect("store"));
    let snapshot = WorkspaceSnapshot {
        graph: PatchGraph {
            nodes: vec![
                node("trunk", "trunk"),
                node("tx", "transcriber"),
                node("log", "decoder_log"),
            ],
            edges: vec![events("trunk", "tx"), events("tx", "log")],
        },
        ..WorkspaceSnapshot::empty()
    };
    let id = store.create_workspace("speech", &snapshot).expect("create");
    store.activate_workspace(id).expect("activate");
    let state = state_over(store);
    let (_, background) = router_with_state(state.clone(), &ServerOptions::default());
    background.detach();
    state
}

#[tokio::test]
async fn a_call_without_a_model_still_goes_on_from_the_transcriber() {
    let state = bench();
    let mut decoded = state.decoded.subscribe();
    let deadline = tokio::time::Instant::now() + GIVE_UP;
    loop {
        let mut sent = record("trunk");
        sent.sinks.clear();
        state.engine.publish_decoded(sent);
        let next = tokio::time::Instant::now() + Duration::from_millis(200);
        let passed = loop {
            match tokio::time::timeout_at(next.min(deadline), decoded.recv()).await {
                Ok(Ok(Decoded::Record(routed)))
                    if routed.record.event.kind == VoiceCall::KIND
                        && routed
                            .record
                            .origin
                            .as_ref()
                            .is_some_and(|o| o.node == "tx") =>
                {
                    break Some(routed);
                }
                Ok(Ok(_) | Err(broadcast::error::RecvError::Lagged(_))) => {}
                Ok(Err(broadcast::error::RecvError::Closed)) => panic!("feed closed"),
                Err(_) => break None,
            }
        };
        if let Some(routed) = passed {
            assert!(routed.record.sinks.contains(&"log".to_owned()));
            assert_eq!(
                routed.record.event.read::<VoiceCall>().unwrap().transcript,
                None
            );
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "nothing came out of the transcriber"
        );
    }
    let status = state.transcribers.snapshot();
    assert_eq!(status.len(), 1);
}

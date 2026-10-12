use sdrmm_wire::{DecoderEvent, RttyText};

use super::*;

fn rtty(text: &str) -> DecoderEvent {
    DecoderEvent::Rtty(RttyText {
        text: text.to_owned(),
    })
}

fn system(node: &str) -> trunking::TrunkSystem {
    trunking::TrunkSystem {
        node: node.to_owned(),
        protocol: Default::default(),
        discovery: Default::default(),
        channel_map: Vec::new(),
        learned: Vec::new(),
        radio: None,
    }
}

async fn next(decoded: &mut broadcast::Receiver<DecodedRecord>) -> DecodedRecord {
    tokio::time::timeout(Duration::from_secs(5), decoded.recv())
        .await
        .expect("a record within five seconds")
        .expect("the decoded stream stays open")
}

#[tokio::test]
async fn a_channel_publishes_with_the_node_it_was_opened_for() {
    let engine = virtual_engine();
    let mut decoded = engine.subscribe_decoded();

    engine
        .decoded_sink(1, 2, Some("channel:rtty"))
        .publish(14_080_000.0, rtty("CQ"));
    engine
        .decoded_sink(1, 3, None)
        .publish(14_080_000.0, rtty("73"));

    let owned = next(&mut decoded).await;
    assert_eq!(
        owned.origin.map(|origin| origin.node).as_deref(),
        Some("channel:rtty")
    );
    assert_eq!((owned.device_set, owned.channel), (1, 2));
    assert_eq!(next(&mut decoded).await.origin, None);
}

#[test]
fn only_records_of_a_configured_trunk_system_reach_the_follower() {
    let engine = virtual_engine();
    engine.configure_trunking(vec![system("trunk")]);

    assert!(engine.trunked(Some("trunk")));
    assert!(!engine.trunked(Some("channel:dmr")));
    assert!(!engine.trunked(None));

    engine.configure_trunking(Vec::new());
    assert!(!engine.trunked(Some("trunk")));
}

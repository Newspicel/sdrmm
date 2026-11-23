use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use str0m::change::{SdpAnswer, SdpOffer};
use str0m::media::{Direction, MediaKind, Mid};
use tokio::sync::mpsc;
use tokio::time::{interval, timeout};

use super::*;

const LOOPBACK: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9);
const PATIENCE: Duration = Duration::from_secs(4);

fn by_text_offer(offer: &SdpOffer) -> SdpOffer {
    SdpOffer::from_sdp_string(&offer.to_sdp_string()).unwrap()
}

fn by_text_answer(answer: &SdpAnswer) -> SdpAnswer {
    SdpAnswer::from_sdp_string(&answer.to_sdp_string()).unwrap()
}

fn sfu_offer(stream: &str, track: &str) -> (Mid, SdpOffer) {
    let mut sfu = new_rtc();
    let mut api = sfu.sdp_api();
    let mid = api.add_media(
        MediaKind::Audio,
        Direction::SendOnly,
        Some(stream.to_owned()),
        Some(track.to_owned()),
        None,
    );
    let (offer, _) = api.apply().unwrap();
    (mid, by_text_offer(&offer))
}

fn frame(index: u32) -> OpusOut {
    let mut payload = vec![0xfc];
    payload.extend_from_slice(&index.to_be_bytes());
    payload.extend((0..40).map(|byte: u8| byte ^ index as u8));
    OpusOut {
        payload,
        timestamp: index * 960,
    }
}

async fn connected_pair() -> (Peer, Peer) {
    let (mut publisher, offer) = Peer::publisher(LOOPBACK).await.unwrap();
    let (mut subscriber, answer) = Peer::subscriber(LOOPBACK, by_text_offer(&offer))
        .await
        .unwrap();
    publisher.accept_answer(by_text_answer(&answer)).unwrap();
    for candidate in subscriber.local_candidates() {
        publisher.add_remote_candidate(candidate);
    }
    for candidate in publisher.local_candidates() {
        subscriber.add_remote_candidate(candidate);
    }
    (publisher, subscriber)
}

#[tokio::test]
async fn two_peers_trade_opus_on_loopback() {
    let (mut publisher, mut subscriber) = connected_pair().await;
    let (mic, mut outgoing) = mpsc::channel(64);
    let (unheard, _unheard_rx) = mpsc::channel(64);
    let (_silence, mut idle) = mpsc::channel(1);
    let (heard, mut incoming) = mpsc::channel(64);
    let sending = tokio::spawn(async move { publisher.run(&mut outgoing, &unheard).await });
    let hearing = tokio::spawn(async move { subscriber.run(&mut idle, &heard).await });
    let talking = tokio::spawn(async move {
        let mut tick = interval(Duration::from_millis(20));
        for index in 0.. {
            tick.tick().await;
            if mic.send(frame(index)).await.is_err() {
                break;
            }
        }
    });

    let first = timeout(PATIENCE, incoming.recv()).await.unwrap().unwrap();
    let second = timeout(PATIENCE, incoming.recv()).await.unwrap().unwrap();

    let index = first.timestamp / 960;
    assert_eq!(first.payload, frame(index).payload);
    assert_eq!(second.timestamp, first.timestamp + 960);
    assert_eq!(second.payload, frame(index + 1).payload);
    assert_eq!(second.mid, first.mid);
    assert_eq!(first.track, None);
    talking.abort();
    sending.abort();
    hearing.abort();
}

#[tokio::test]
async fn subscriber_answers_an_offer_with_audio() {
    let (_, offer) = sfu_offer("PA_alice|TR_alice", "TR_alice");
    let started = Instant::now();

    let (peer, answer) = Peer::subscriber(LOOPBACK, offer).await.unwrap();

    let sdp = by_text_answer(&answer).to_sdp_string();
    assert!(sdp.contains("m=audio"), "{sdp}");
    assert!(sdp.contains("a=recvonly"), "{sdp}");
    assert!(sdp.contains("opus/48000/2"), "{sdp}");
    let candidates = peer.local_candidates();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].addr().ip(), LOOPBACK.ip());
    assert_eq!(peer.dropped(), 0);
    assert!(started.elapsed() < PATIENCE);
}

#[tokio::test]
async fn the_track_sid_comes_from_msid() {
    let (mid, offer) = sfu_offer("PA_alice|TR_alice", "TR_alice");
    let (peer, _) = Peer::subscriber(LOOPBACK, offer).await.unwrap();

    assert_eq!(peer.track_of(mid).as_deref(), Some("TR_alice"));
    assert_eq!(
        track_sid("PA_bob|TR_firefox", "{local-id}"),
        Some("TR_firefox")
    );
    assert_eq!(track_sid("PA_bob|stream", "TR_bob"), Some("TR_bob"));
    assert_eq!(track_sid("stream", "track"), None);
}

#[tokio::test]
async fn the_publisher_offers_a_send_only_mic_and_data() {
    let (_, offer) = Peer::publisher(LOOPBACK).await.unwrap();

    let sdp = by_text_offer(&offer).to_sdp_string();
    assert!(sdp.contains("m=audio"), "{sdp}");
    assert!(sdp.contains("a=sendonly"), "{sdp}");
    assert!(sdp.contains(" mic"), "{sdp}");
    assert!(sdp.contains("m=application"), "{sdp}");
}

#[tokio::test]
async fn an_answer_with_no_offer_is_refused() {
    let (_, offer) = sfu_offer("PA_alice|TR_alice", "TR_alice");
    let (mut peer, answer) = Peer::subscriber(LOOPBACK, offer).await.unwrap();

    assert!(peer.accept_answer(answer).is_err());
}

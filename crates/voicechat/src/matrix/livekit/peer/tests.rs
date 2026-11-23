use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use str0m::change::{SdpAnswer, SdpOffer};
use str0m::media::{Direction, MediaKind, Mid};
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
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

fn talk(
    mut publisher: Peer,
    unheard: mpsc::Sender<OpusIn>,
) -> (JoinHandle<Result<(), String>>, JoinHandle<()>) {
    let (mic, mut outgoing) = mpsc::channel(64);
    let sending = tokio::spawn(async move { publisher.run(&mut outgoing, &unheard).await });
    let talking = tokio::spawn(async move {
        let mut tick = interval(Duration::from_millis(20));
        for index in 0.. {
            tick.tick().await;
            if mic.send(frame(index)).await.is_err() {
                break;
            }
        }
    });
    (sending, talking)
}

#[tokio::test]
async fn two_peers_trade_opus_on_loopback() {
    let (publisher, mut subscriber) = connected_pair().await;
    let (unheard, _unheard_rx) = mpsc::channel(64);
    let (_silence, mut idle) = mpsc::channel(1);
    let (heard, mut incoming) = mpsc::channel(64);
    let (sending, talking) = talk(publisher, unheard);
    let hearing = tokio::spawn(async move { subscriber.run(&mut idle, &heard).await });

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
async fn a_peer_is_up_once_media_can_flow() {
    let (publisher, mut subscriber) = connected_pair().await;
    let mut up = publisher.up();
    assert!(!*up.borrow());
    let (unheard, _unheard_rx) = mpsc::channel(64);
    let (_silence, mut idle) = mpsc::channel(1);
    let (heard, _incoming) = mpsc::channel(64);
    let (sending, talking) = talk(publisher, unheard);
    let hearing = tokio::spawn(async move { subscriber.run(&mut idle, &heard).await });

    timeout(PATIENCE, up.wait_for(|up| *up))
        .await
        .unwrap()
        .unwrap();

    talking.abort();
    sending.abort();
    hearing.abort();
}

#[tokio::test]
async fn subscriber_answers_an_offer_with_audio() {
    let (_, offer) = sfu_offer("PA_alice|TR_alice", "TR_alice");

    let (peer, answer) = Peer::subscriber(LOOPBACK, offer).await.unwrap();

    let sdp = by_text_answer(&answer).to_sdp_string();
    assert!(sdp.contains("m=audio"), "{sdp}");
    assert!(sdp.contains("a=recvonly"), "{sdp}");
    assert!(sdp.contains("opus/48000/2"), "{sdp}");
    let candidates = peer.local_candidates();
    assert_eq!(candidates.len(), 2);
    assert!(candidates.iter().all(|c| c.addr().ip() == LOOPBACK.ip()));
    assert!(sdp.contains("tcptype active"), "{sdp}");
    assert_eq!(peer.bundle(), first_mid(&sdp));
}

#[tokio::test]
async fn a_reader_that_falls_behind_counts_drops() {
    let (publisher, mut subscriber) = connected_pair().await;
    let (unheard, _unheard_rx) = mpsc::channel(1);
    let (_silence, mut idle) = mpsc::channel(1);
    let (heard, _never_read) = mpsc::channel(1);
    let (sending, talking) = talk(publisher, unheard);
    let started = Instant::now();

    while subscriber.dropped() == 0 && started.elapsed() < PATIENCE {
        let _ = timeout(Duration::from_millis(50), subscriber.run(&mut idle, &heard)).await;
    }

    assert!(subscriber.dropped() > 0);
    talking.abort();
    sending.abort();
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
    let (peer, offer) = Peer::publisher(LOOPBACK).await.unwrap();

    let sdp = by_text_offer(&offer).to_sdp_string();
    assert!(peer.bundle().is_some());
    assert_eq!(peer.bundle(), first_mid(&sdp));
    assert!(sdp.contains("m=audio"), "{sdp}");
    assert!(sdp.contains("a=sendonly"), "{sdp}");
    assert!(sdp.contains(" mic"), "{sdp}");
    assert!(sdp.contains("m=application"), "{sdp}");
}

#[tokio::test]
async fn an_answering_peer_sends_checks() {
    let sfu = tokio::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let (_, offer) = sfu_offer("PA_alice|TR_alice", "TR_alice");
    let (mut peer, _) = Peer::subscriber(LOOPBACK, offer).await.unwrap();
    peer.add_remote_candidate(Candidate::host(sfu.local_addr().unwrap(), "udp").unwrap());
    let (_silence, mut idle) = mpsc::channel(1);
    let (heard, _never_read) = mpsc::channel(1);
    let running = tokio::spawn(async move { peer.run(&mut idle, &heard).await });

    let mut buf = [0; DATAGRAM];
    let (len, _) = timeout(PATIENCE, sfu.recv_from(&mut buf))
        .await
        .unwrap()
        .unwrap();

    assert!(len >= 20);
    assert_eq!(buf[4..8], [0x21, 0x12, 0xa4, 0x42]);
    running.abort();
}

#[tokio::test]
async fn a_passive_tcp_candidate_is_dialled_for_checks() {
    let sfu = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let passive = Candidate::builder()
        .tcp()
        .host(sfu.local_addr().unwrap())
        .tcptype(TcpType::Passive)
        .build()
        .unwrap();
    let (_, offer) = sfu_offer("PA_alice|TR_alice", "TR_alice");
    let (mut peer, _) = Peer::subscriber(LOOPBACK, offer).await.unwrap();
    peer.add_remote_candidate(passive);
    let (_silence, mut idle) = mpsc::channel(1);
    let (heard, _never_read) = mpsc::channel(1);
    let running = tokio::spawn(async move { peer.run(&mut idle, &heard).await });

    let (mut stream, _) = timeout(PATIENCE, sfu.accept()).await.unwrap().unwrap();
    let mut head = [0; 10];
    stream.read_exact(&mut head).await.unwrap();

    assert!(u16::from_be_bytes([head[0], head[1]]) >= 20);
    assert_eq!(head[6..10], [0x21, 0x12, 0xa4, 0x42]);
    running.abort();
}

#[tokio::test]
async fn ice_down_past_its_grace_loses_the_call() {
    let (_, offer) = sfu_offer("PA_alice|TR_alice", "TR_alice");
    let (mut peer, _) = Peer::subscriber(LOOPBACK, offer).await.unwrap();
    peer.down_since = Instant::now().checked_sub(ICE_GRACE);
    let (_silence, mut idle) = mpsc::channel(1);
    let (heard, _never_read) = mpsc::channel(1);

    let ran = timeout(PATIENCE, peer.run(&mut idle, &heard))
        .await
        .unwrap();

    assert_eq!(ran, Err(lost()));
}

#[tokio::test]
async fn an_answer_with_no_offer_is_refused() {
    let (_, offer) = sfu_offer("PA_alice|TR_alice", "TR_alice");
    let (mut peer, answer) = Peer::subscriber(LOOPBACK, offer).await.unwrap();

    assert!(peer.accept_answer(answer).is_err());
}

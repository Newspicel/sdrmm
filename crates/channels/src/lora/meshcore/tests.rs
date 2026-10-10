use sdrmm_wire::{LoraKey, MeshcoreContent, MeshcoreNodeType, MeshcorePayloadType, MeshcoreRoute};

use super::super::crypto;
use super::encode::group_datagram;
use super::keys::hashtag_secret;
use super::{Keys, PUBLIC_SECRET, advert, decode, group_text};

const SEED: [u8; 32] = [7; 32];

fn hashtag_keys(name: &str) -> Keys {
    Keys::new(&[LoraKey::MeshcoreChannel {
        name: name.to_owned(),
        secret: String::new(),
    }])
    .unwrap()
}

fn content(bytes: &[u8], keys: &Keys) -> MeshcoreContent {
    decode(bytes, keys).unwrap().content
}

fn group_parts(content: MeshcoreContent) -> (Option<String>, Option<String>, Option<String>) {
    match content {
        MeshcoreContent::GroupText {
            channel,
            sender,
            text,
            ..
        } => (channel, sender, text),
        other => panic!("not group text: {other:?}"),
    }
}

#[test]
fn advert_round_trip_verifies_signature_and_location() {
    let bytes = advert(
        &SEED,
        1_700_000_000,
        1,
        "Alice",
        Some((52.520_008, -13.404_954)),
    );
    let packet = decode(&bytes, &Keys::default()).unwrap();
    assert_eq!(packet.route, MeshcoreRoute::Flood);
    assert_eq!(packet.payload_type, MeshcorePayloadType::Advert);
    assert!(packet.path.is_empty());
    let MeshcoreContent::Advert {
        public_key,
        timestamp,
        node_type,
        name,
        lat,
        lon,
        signature_ok,
    } = packet.content
    else {
        panic!("not an advert");
    };
    assert_eq!(
        public_key,
        crate::datalink::hex(&crypto::ed25519_keypair(&SEED))
    );
    assert_eq!(timestamp, 1_700_000_000);
    assert_eq!(node_type, MeshcoreNodeType::Chat);
    assert_eq!(name.as_deref(), Some("Alice"));
    assert!((lat.unwrap() - 52.520_008).abs() < 1e-9);
    assert!((lon.unwrap() + 13.404_954).abs() < 1e-9);
    assert!(signature_ok);
}

#[test]
fn advert_without_location_has_name_and_type() {
    let bytes = advert(&SEED, 5, 2, "Repeater one", None);
    let MeshcoreContent::Advert {
        node_type,
        name,
        lat,
        lon,
        signature_ok,
        ..
    } = content(&bytes, &Keys::default())
    else {
        panic!("not an advert");
    };
    assert_eq!(node_type, MeshcoreNodeType::Repeater);
    assert_eq!(name.as_deref(), Some("Repeater one"));
    assert_eq!((lat, lon), (None, None));
    assert!(signature_ok);
}

#[test]
fn advert_with_corrupted_signature_is_flagged() {
    let mut bytes = advert(&SEED, 9, 3, "Room", None);
    bytes[2 + 32 + 4 + 10] ^= 0x01;
    let MeshcoreContent::Advert {
        node_type,
        signature_ok,
        ..
    } = content(&bytes, &Keys::default())
    else {
        panic!("not an advert");
    };
    assert_eq!(node_type, MeshcoreNodeType::Room);
    assert!(!signature_ok);
}

#[test]
fn advert_with_tampered_name_fails_signature() {
    let mut bytes = advert(&SEED, 9, 4, "Sensor", None);
    let last = bytes.len() - 1;
    bytes[last] = b'X';
    let MeshcoreContent::Advert {
        node_type,
        name,
        signature_ok,
        ..
    } = content(&bytes, &Keys::default())
    else {
        panic!("not an advert");
    };
    assert_eq!(node_type, MeshcoreNodeType::Sensor);
    assert_eq!(name.as_deref(), Some("SensoX"));
    assert!(!signature_ok);
}

#[test]
fn advert_name_is_cut_to_firmware_limit() {
    let bytes = advert(&SEED, 1, 1, &"n".repeat(60), Some((1.0, 2.0)));
    let MeshcoreContent::Advert {
        name, signature_ok, ..
    } = content(&bytes, &Keys::default())
    else {
        panic!("not an advert");
    };
    assert_eq!(name.map(|name| name.len()), Some(32 - 9));
    assert!(signature_ok);
}

#[test]
fn public_channel_group_text_round_trip() {
    let bytes = group_text(&PUBLIC_SECRET, 1_234, "bob", "hello: world");
    let packet = decode(&bytes, &Keys::default()).unwrap();
    assert_eq!(packet.payload_type, MeshcorePayloadType::GroupText);
    let MeshcoreContent::GroupText {
        channel_hash,
        channel,
        timestamp,
        sender,
        text,
    } = packet.content
    else {
        panic!("not group text");
    };
    assert_eq!(channel_hash, crypto::sha256(&[&PUBLIC_SECRET])[0]);
    assert_eq!(channel.as_deref(), Some("Public"));
    assert_eq!(timestamp, Some(1_234));
    assert_eq!(sender.as_deref(), Some("bob"));
    assert_eq!(text.as_deref(), Some("hello: world"));
}

#[test]
fn public_secret_matches_firmware_base64() {
    assert_eq!(
        crypto::base64("izOH6cXN6mrJ5e26oRXNcg==").as_deref(),
        Some(PUBLIC_SECRET.as_slice())
    );
}

#[test]
fn hashtag_secret_matches_companion_protocol_vector() {
    assert_eq!(
        crypto::hex("9cd8fcf22a47333b591d96a2b848b73f").as_deref(),
        Some(hashtag_secret("#test").as_slice())
    );
}

#[test]
fn hashtag_channel_round_trip() {
    let bytes = group_text(&hashtag_secret("#test"), 77, "carol", "hi");
    let (channel, sender, text) = group_parts(content(&bytes, &hashtag_keys("#test")));
    assert_eq!(channel.as_deref(), Some("#test"));
    assert_eq!(sender.as_deref(), Some("carol"));
    assert_eq!(text.as_deref(), Some("hi"));
    assert_eq!(
        group_parts(content(&bytes, &Keys::default())),
        (None, None, None)
    );
}

#[test]
fn long_secret_channel_round_trip() {
    let secret: Vec<u8> = (1..=32).collect();
    let keys = Keys::new(&[LoraKey::MeshcoreChannel {
        name: "Ops".to_owned(),
        secret: crate::datalink::hex(&secret),
    }])
    .unwrap();
    let bytes = group_text(&secret, 1, "dave", "x".repeat(100).as_str());
    let (channel, sender, text) = group_parts(content(&bytes, &keys));
    assert_eq!(channel.as_deref(), Some("Ops"));
    assert_eq!(sender.as_deref(), Some("dave"));
    assert_eq!(text, Some("x".repeat(100)));
}

#[test]
fn group_text_without_separator_has_no_sender() {
    let plaintext = [&[1, 0, 0, 0, 0][..], b"just text\0junk"].concat();
    let bytes = group_datagram(0x05, &PUBLIC_SECRET, &plaintext);
    assert_eq!(
        group_parts(content(&bytes, &Keys::default())),
        (
            Some("Public".to_owned()),
            None,
            Some("just text".to_owned())
        )
    );
}

#[test]
fn group_data_round_trip() {
    let plaintext = [0x34, 0x12, 3, 0xaa, 0xbb, 0xcc];
    let bytes = group_datagram(0x06, &PUBLIC_SECRET, &plaintext);
    assert_eq!(
        content(&bytes, &Keys::default()),
        MeshcoreContent::GroupData {
            channel_hash: bytes[2],
            channel: Some("Public".to_owned()),
            data_type: Some(0x1234),
            data: Some("aabbcc".to_owned()),
        }
    );
}

#[test]
fn wrong_mac_leaves_channel_unknown() {
    let mut bytes = group_text(&PUBLIC_SECRET, 1, "eve", "tampered");
    bytes[3] ^= 0xff;
    let MeshcoreContent::GroupText {
        channel_hash,
        channel,
        timestamp,
        sender,
        text,
    } = content(&bytes, &Keys::default())
    else {
        panic!("not group text");
    };
    assert_eq!(channel_hash, bytes[2]);
    assert_eq!((channel, timestamp, sender, text), (None, None, None, None));
}

#[test]
fn keys_parse_base64_hex_and_hashtag() {
    let keys = [
        LoraKey::MeshcoreChannel {
            name: "b64".to_owned(),
            secret: "izOH6cXN6mrJ5e26oRXNcg==".to_owned(),
        },
        LoraKey::MeshcoreChannel {
            name: "hex".to_owned(),
            secret: "8b3387e9c5cdea6ac9e5edbaa115cd72".to_owned(),
        },
        LoraKey::MeshcoreChannel {
            name: "#mesh".to_owned(),
            secret: String::new(),
        },
        LoraKey::MeshtasticChannel {
            name: "ignored".to_owned(),
            psk: "garbage".to_owned(),
        },
    ];
    assert!(Keys::new(&keys).is_ok());
}

#[test]
fn keys_reject_malformed_secrets() {
    for secret in [
        "",
        "abcd",
        "not base64!",
        "AAAA",
        "8b3387e9c5cdea6ac9e5edbaa115cd",
    ] {
        let key = LoraKey::MeshcoreChannel {
            name: "bad".to_owned(),
            secret: secret.to_owned(),
        };
        assert!(Keys::new(&[key]).is_err(), "{secret}");
    }
}

#[test]
fn transport_codes_and_two_byte_path_hashes() {
    let bytes = [
        0x0c, 0x34, 0x12, 0x78, 0x56, 0x42, 0xaa, 0xbb, 0xcc, 0xdd, 0x01, 0x02, 0x03, 0x04,
    ];
    let packet = decode(&bytes, &Keys::default()).unwrap();
    assert_eq!(packet.route, MeshcoreRoute::TransportFlood);
    assert_eq!(packet.payload_type, MeshcorePayloadType::Ack);
    assert_eq!(packet.transport_codes, vec![0x1234, 0x5678]);
    assert_eq!(packet.path, vec!["aabb", "ccdd"]);
    assert_eq!(
        packet.content,
        MeshcoreContent::Ack {
            checksum: 0x0403_0201
        }
    );
}

#[test]
fn three_byte_path_hashes_on_transport_direct() {
    let bytes = [0x2f, 0, 0, 0, 0, 0x81, 1, 2, 3, 0xde, 0xad];
    let packet = decode(&bytes, &Keys::default()).unwrap();
    assert_eq!(packet.route, MeshcoreRoute::TransportDirect);
    assert_eq!(packet.payload_type, MeshcorePayloadType::Control);
    assert_eq!(packet.path, vec!["010203"]);
    assert_eq!(
        packet.content,
        MeshcoreContent::Control {
            subtype: 0x0d,
            data: "ad".to_owned()
        }
    );
}

#[test]
fn trace_reports_snr_path_and_hop_hashes() {
    let bytes = [
        0x26, 0x02, 0x1d, 0xec, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0xa1, 0xb2,
        0xc3, 0xd4,
    ];
    let packet = decode(&bytes, &Keys::default()).unwrap();
    assert_eq!(packet.route, MeshcoreRoute::Direct);
    assert_eq!(packet.path, vec!["7.25", "-5.00"]);
    assert_eq!(
        packet.content,
        MeshcoreContent::Trace {
            tag: 1,
            auth_code: 2,
            flags: 1,
            hops: vec!["a1b2".to_owned(), "c3d4".to_owned()],
        }
    );
}

#[test]
fn opaque_payload_types() {
    let block = [0x55; 16];
    let encrypted = [&[0x0a, 0x00, 0x12, 0x34, 0xab, 0xcd][..], &block].concat();
    assert_eq!(
        content(&encrypted, &Keys::default()),
        MeshcoreContent::Encrypted {
            destination: "12".to_owned(),
            source: "34".to_owned(),
            mac: "abcd".to_owned(),
        }
    );
    let anon = [&[0x1e, 0x00, 0x99][..], &[0x11; 32], &[0, 0], &block].concat();
    assert_eq!(
        content(&anon, &Keys::default()),
        MeshcoreContent::AnonRequest {
            destination: "99".to_owned(),
            public_key: "11".repeat(32),
        }
    );
    assert_eq!(
        content(
            &[0x29, 0x00, 0x23, 0x01, 0x02, 0x03, 0x04],
            &Keys::default()
        ),
        MeshcoreContent::Multipart {
            remaining: 2,
            inner_type: 3,
            data: "01020304".to_owned(),
        }
    );
    assert_eq!(
        content(&[0x3e, 0x00, 0xfe], &Keys::default()),
        MeshcoreContent::Raw {
            data: "fe".to_owned()
        }
    );
    let reserved = decode(&[0x31, 0x00, 0x01], &Keys::default()).unwrap();
    assert_eq!(reserved.payload_type, MeshcorePayloadType::Reserved);
}

#[test]
fn malformed_packets_are_rejected() {
    let keys = Keys::default();
    let ack = [0x0d, 0x00, 1, 2, 3, 4];
    assert!(decode(&ack, &keys).is_some());
    let advert_bytes = advert(&SEED, 1, 1, "A", None);
    let group = group_text(&PUBLIC_SECRET, 1, "a", "b");
    let cases: Vec<Vec<u8>> = vec![
        vec![],
        vec![0x4d, 0x00, 1, 2, 3, 4],
        vec![0x8d, 0x00, 1, 2, 3, 4],
        vec![0x0d, 0xc1, 0xaa, 1, 2, 3, 4],
        vec![0x0d, 0x41, 0xaa, 1, 2, 3, 4],
        vec![0x0d, 0x00],
        vec![0x0d],
        vec![0x0c, 0x00, 0x00, 0x00],
        vec![0x0d, 0x00, 1, 2, 3],
        vec![0x0d, 0x00, 1, 2, 3, 4, 5],
        [&[0x0d, 0x61][..], &[0; 66], &[1, 2, 3, 4]].concat(),
        [&[0x3d, 0x00][..], &[0; 185]].concat(),
        advert_bytes[..advert_bytes.len() - 2].to_vec(),
        advert_bytes[..2 + 32 + 4 + 64].to_vec(),
        group[..group.len() - 1].to_vec(),
        group[..5].to_vec(),
        vec![0x0a, 0x00, 1, 2, 3, 4],
        vec![0x26, 0x00, 1, 2, 3, 4, 5, 6, 7, 8, 1, 0xaa],
        vec![0x29, 0x00, 0x23, 0x01],
    ];
    for (index, bytes) in cases.iter().enumerate() {
        assert_eq!(decode(bytes, &keys), None, "case {index}");
    }
}

#[test]
fn path_at_limit_is_accepted() {
    let bytes = [&[0x0d, 0x80 | 21][..], &[0x11; 63], &[1, 2, 3, 4]].concat();
    assert_eq!(decode(&bytes, &Keys::default()).unwrap().path.len(), 21);
    let bytes = [&[0x0d, 0x80 | 22][..], &[0x11; 66], &[1, 2, 3, 4]].concat();
    assert_eq!(decode(&bytes, &Keys::default()), None);
}

#[test]
fn random_bytes_never_panic() {
    let keys = hashtag_keys("#fuzz");
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..5000 {
        let len = (next() % 260) as usize;
        let mut bytes: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        if let Some(header) = bytes.first_mut() {
            *header &= 0x3f;
        }
        let _ = decode(&bytes, &keys);
    }
}

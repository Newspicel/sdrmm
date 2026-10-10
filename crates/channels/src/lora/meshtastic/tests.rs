use sdrmm_wire::{LoraKey, MeshtasticContent, MeshtasticEncryption, MeshtasticPacket};

use super::proto::{Value, Writer, each_field, repeated_fixed32, repeated_varint};
use super::synth::packet;
use super::{DEFAULT_PSK, Keys, channel_hash, decode, expand_psk, position, text};

const NODE: u32 = 0xa1b2_c3d4;
const SECRET_PSK: &str = "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA=";

fn secret_psk() -> Vec<u8> {
    (1..=32).collect()
}

fn data(port: u32, payload: &[u8]) -> Vec<u8> {
    Writer::default()
        .varint(1, u64::from(port))
        .bytes(2, payload)
        .finish()
}

fn decoded(payload: &[u8]) -> MeshtasticPacket {
    decode(payload, &Keys::default()).unwrap()
}

#[test]
fn default_channel_hashes_match_the_community_values() {
    let key = expand_psk(&[1]).unwrap();
    assert_eq!(key, DEFAULT_PSK);
    assert_eq!(channel_hash("LongFast", &key), 0x08);
    assert_eq!(channel_hash("MediumFast", &key), 0x1f);
    assert_eq!(expand_psk(&[2]).unwrap()[15], 0x02);
    assert!(expand_psk(&[0]).unwrap().is_empty());
    assert_eq!(expand_psk(&[7, 7]).unwrap().len(), 16);
    assert_eq!(expand_psk(&[7; 17]).unwrap().len(), 32);
    assert!(expand_psk(&[7; 33]).is_none());
}

#[test]
fn text_round_trips_on_long_fast() {
    let payload = text(
        NODE,
        MeshtasticPacket::BROADCAST,
        0x1234_5678,
        "LongFast",
        &[1],
        "hello mesh",
    );
    assert_eq!(payload[13], 0x08);
    let packet = decoded(&payload);
    assert_eq!(packet.from, NODE);
    assert_eq!(packet.to, MeshtasticPacket::BROADCAST);
    assert_eq!(packet.id, 0x1234_5678);
    assert_eq!((packet.hop_limit, packet.hop_start), (3, 3));
    assert!(!packet.want_ack && !packet.via_mqtt);
    assert_eq!(packet.encryption, MeshtasticEncryption::Channel);
    assert_eq!(packet.channel.as_deref(), Some("LongFast"));
    assert_eq!(packet.port, Some(1));
    assert_eq!(packet.port_name.as_deref(), Some("TEXT_MESSAGE_APP"));
    assert_eq!(
        packet.content,
        Some(MeshtasticContent::Text {
            text: "hello mesh".to_owned()
        })
    );
    assert_ne!(&payload[16..], data(1, b"hello mesh").as_slice());
}

#[test]
fn position_round_trips_on_medium_fast() {
    let payload = position(NODE, 9, "MediumFast", &[1], 47.376_887, -8.541_694, -12);
    let packet = decoded(&payload);
    assert_eq!(packet.channel.as_deref(), Some("MediumFast"));
    assert_eq!(packet.port_name.as_deref(), Some("POSITION_APP"));
    let Some(MeshtasticContent::Position {
        lat,
        lon,
        altitude_m,
        ..
    }) = packet.content
    else {
        panic!("not a position: {:?}", packet.content);
    };
    assert!((lat.unwrap() - 47.376_887).abs() < 1e-7);
    assert!((lon.unwrap() + 8.541_694).abs() < 1e-7);
    assert_eq!(altitude_m, Some(-12));
}

#[test]
fn a_user_channel_with_a_long_psk_decrypts() {
    let payload = text(NODE, 0x0102_0304, 77, "Secret", &secret_psk(), "psst");
    let keys = Keys::new(&[LoraKey::MeshtasticChannel {
        name: "Secret".to_owned(),
        psk: SECRET_PSK.to_owned(),
    }])
    .unwrap();
    let packet = decode(&payload, &keys).unwrap();
    assert_eq!(packet.encryption, MeshtasticEncryption::Channel);
    assert_eq!(packet.channel.as_deref(), Some("Secret"));
    assert_eq!(
        packet.content,
        Some(MeshtasticContent::Text {
            text: "psst".to_owned()
        })
    );
}

#[test]
fn an_unknown_key_leaves_the_packet_encrypted() {
    let payload = text(
        NODE,
        MeshtasticPacket::BROADCAST,
        77,
        "Secret",
        &secret_psk(),
        "psst",
    );
    let packet = decoded(&payload);
    assert_eq!(packet.encryption, MeshtasticEncryption::Unknown);
    assert_eq!(packet.content, None);
    assert_eq!(packet.port, None);
    assert_eq!(packet.channel, None);
}

#[test]
fn open_channels_carry_plain_data() {
    let payload = text(NODE, MeshtasticPacket::BROADCAST, 5, "Open", &[0], "clear");
    assert_eq!(&payload[16..], data(1, b"clear").as_slice());
    let keys = Keys::new(&[LoraKey::MeshtasticChannel {
        name: "Open".to_owned(),
        psk: "AA==".to_owned(),
    }])
    .unwrap();
    let packet = decode(&payload, &keys).unwrap();
    assert_eq!(packet.encryption, MeshtasticEncryption::Open);
    assert_eq!(packet.channel.as_deref(), Some("Open"));
}

#[test]
fn direct_messages_without_a_channel_hash_look_like_pki() {
    let mut payload = vec![0; 16];
    payload[..4].copy_from_slice(&0x0102_0304_u32.to_le_bytes());
    payload[4..8].copy_from_slice(&NODE.to_le_bytes());
    payload[12] = 0x0b;
    payload.extend_from_slice(&[0x5a; 30]);
    let packet = decoded(&payload);
    assert_eq!(packet.encryption, MeshtasticEncryption::Pki);
    assert!(packet.want_ack);
    assert_eq!(packet.content, None);
    payload[..4].copy_from_slice(&MeshtasticPacket::BROADCAST.to_le_bytes());
    assert_eq!(decoded(&payload).encryption, MeshtasticEncryption::Unknown);
    payload[..4].copy_from_slice(&0x0102_0304_u32.to_le_bytes());
    assert_eq!(
        decoded(&payload[..28]).encryption,
        MeshtasticEncryption::Unknown
    );
}

#[test]
fn node_info_is_read_from_a_user() {
    let user = Writer::default()
        .bytes(1, b"!a1b2c3d4")
        .bytes(2, b"Base camp")
        .bytes(3, b"BC")
        .varint(5, 43)
        .varint(6, 1)
        .varint(7, 2)
        .bytes(8, &[0xab; 32])
        .finish();
    let payload = packet(
        NODE,
        MeshtasticPacket::BROADCAST,
        1,
        "LongFast",
        &[1],
        data(4, &user),
    );
    let packet = decoded(&payload);
    let Some(MeshtasticContent::NodeInfo {
        id,
        long_name,
        short_name,
        hw_model,
        role,
        licensed,
        public_key,
    }) = packet.content
    else {
        panic!("not node info: {:?}", packet.content);
    };
    assert_eq!(
        (id.as_str(), long_name.as_str(), short_name.as_str()),
        ("!a1b2c3d4", "Base camp", "BC")
    );
    assert_eq!((hw_model, role, licensed), (Some(43), Some(2), true));
    assert_eq!(public_key, Some("ab".repeat(32)));
}

#[test]
fn device_telemetry_lists_present_metrics() {
    let metrics = Writer::default()
        .varint(1, 87)
        .fixed32(2, 4.15_f32.to_bits())
        .fixed32(3, 12.5_f32.to_bits())
        .finish();
    let telemetry = Writer::default()
        .fixed32(1, 1_700_000_000)
        .bytes(2, &metrics)
        .finish();
    let payload = packet(
        NODE,
        MeshtasticPacket::BROADCAST,
        2,
        "LongFast",
        &[1],
        data(67, &telemetry),
    );
    let packet = decoded(&payload);
    assert_eq!(packet.port_name.as_deref(), Some("TELEMETRY_APP"));
    let Some(MeshtasticContent::Telemetry {
        kind,
        time,
        metrics,
    }) = packet.content
    else {
        panic!("not telemetry: {:?}", packet.content);
    };
    assert_eq!(kind, "device");
    assert_eq!(time, Some(1_700_000_000));
    let named: Vec<(&str, f64)> = metrics
        .iter()
        .map(|metric| (metric.name.as_str(), metric.value))
        .collect();
    assert_eq!(
        named,
        [
            ("battery_level", 87.0),
            ("voltage", 4.15),
            ("channel_utilization", 12.5)
        ]
    );
}

#[test]
fn negative_local_stats_survive_ten_byte_varints() {
    let stats = Writer::default().int32(15, -110).finish();
    assert_eq!(stats.len(), 11);
    let telemetry = Writer::default().bytes(6, &stats).finish();
    let content = super::telemetry::telemetry(&telemetry).unwrap();
    let MeshtasticContent::Telemetry { kind, metrics, .. } = content else {
        panic!("not telemetry");
    };
    assert_eq!(kind, "local_stats");
    assert_eq!(metrics[0].name, "noise_floor");
    assert_eq!(metrics[0].value, -110.0);
}

#[test]
fn traceroutes_accept_packed_and_unpacked_fields() {
    let packed_route: Vec<u8> = [0x1111_1111_u32, 0x2222_2222]
        .iter()
        .flat_map(|node| node.to_le_bytes())
        .collect();
    let packed_snr: Vec<u8> = [24, 0, -20]
        .iter()
        .flat_map(|snr| Writer::default().int32(1, *snr).finish().split_off(1))
        .collect();
    let discovery = Writer::default()
        .bytes(1, &packed_route)
        .bytes(2, &packed_snr)
        .fixed32(3, 0x3333_3333)
        .fixed32(3, 0x4444_4444)
        .int32(4, -8)
        .finish();
    let payload = packet(NODE, 0x1111_1111, 3, "LongFast", &[1], data(70, &discovery));
    let packet = decoded(&payload);
    assert_eq!(
        packet.content,
        Some(MeshtasticContent::Traceroute {
            route: vec![0x1111_1111, 0x2222_2222],
            snr_towards_db: vec![6.0, 0.0, -5.0],
            route_back: vec![0x3333_3333, 0x4444_4444],
            snr_back_db: vec![-2.0],
        })
    );
}

#[test]
fn routing_reports_errors_and_acks() {
    let nak = Writer::default().varint(3, 1).finish();
    let content = decoded(&packet(NODE, 1, 4, "LongFast", &[1], data(5, &nak))).content;
    assert_eq!(
        content,
        Some(MeshtasticContent::Routing {
            error: Some("NO_ROUTE".to_owned()),
            route: Vec::new()
        })
    );
    let mut reply = Writer::default().varint(1, 5).bytes(2, &[]).finish();
    reply.extend(Writer::default().fixed32(6, 99).finish());
    let packet = decoded(&packet(NODE, 1, 5, "LongFast", &[1], reply));
    assert_eq!(packet.request_id, Some(99));
    assert_eq!(
        packet.content,
        Some(MeshtasticContent::Routing {
            error: None,
            route: Vec::new()
        })
    );
}

#[test]
fn neighbors_waypoints_and_map_reports_parse() {
    let neighbor = Writer::default()
        .varint(1, 0x0102_0304)
        .fixed32(2, 7.5_f32.to_bits())
        .finish();
    let info = Writer::default()
        .varint(1, u64::from(NODE))
        .bytes(4, &neighbor)
        .finish();
    let content = decoded(&packet(
        NODE,
        u32::MAX,
        6,
        "LongFast",
        &[1],
        data(71, &info),
    ))
    .content;
    let Some(MeshtasticContent::NeighborInfo { node, neighbors }) = content else {
        panic!("not neighbor info");
    };
    assert_eq!(
        (
            node,
            neighbors.len(),
            neighbors[0].node,
            neighbors[0].snr_db
        ),
        (NODE, 1, 0x0102_0304, 7.5)
    );
    let waypoint = Writer::default()
        .varint(1, 42)
        .sfixed32(2, 475_000_000)
        .sfixed32(3, -1_000_000)
        .bytes(6, b"Hut")
        .finish();
    let content = decoded(&packet(
        NODE,
        u32::MAX,
        7,
        "LongFast",
        &[1],
        data(8, &waypoint),
    ))
    .content;
    let Some(MeshtasticContent::Waypoint {
        id, name, lat, lon, ..
    }) = content
    else {
        panic!("not a waypoint");
    };
    assert_eq!(
        (id, name.as_str(), lat, lon),
        (42, "Hut", Some(47.5), Some(-0.1))
    );
    let report = Writer::default()
        .bytes(1, b"Relay")
        .bytes(5, b"2.6.0")
        .int32(11, 512)
        .varint(13, 9)
        .finish();
    let content = decoded(&packet(
        NODE,
        u32::MAX,
        8,
        "LongFast",
        &[1],
        data(73, &report),
    ))
    .content;
    let Some(MeshtasticContent::MapReport {
        long_name,
        firmware_version,
        altitude_m,
        online_nodes,
        ..
    }) = content
    else {
        panic!("not a map report");
    };
    assert_eq!(
        (
            long_name.as_str(),
            firmware_version.as_str(),
            altitude_m,
            online_nodes
        ),
        ("Relay", "2.6.0", Some(512), Some(9))
    );
}

#[test]
fn unknown_ports_and_broken_payloads_fall_back_to_bytes() {
    let content = decoded(&packet(
        NODE,
        u32::MAX,
        9,
        "LongFast",
        &[1],
        data(66, b"seq 1"),
    ))
    .content;
    assert_eq!(
        content,
        Some(MeshtasticContent::Data {
            bytes: "7365712031".to_owned()
        })
    );
    let content = decoded(&packet(
        NODE,
        u32::MAX,
        10,
        "LongFast",
        &[1],
        data(3, &[0x0d, 1]),
    ))
    .content;
    assert_eq!(
        content,
        Some(MeshtasticContent::Data {
            bytes: "0d01".to_owned()
        })
    );
}

#[test]
fn the_reader_skips_unknown_fields_and_rejects_garbage() {
    let mut seen = Vec::new();
    let message = [
        0x08, 0x96, 0x01, 0x11, 1, 2, 3, 4, 5, 6, 7, 8, 0x1b, 0x08, 0x01, 0x1c, 0x25, 1, 0, 0, 0,
    ];
    assert!(
        each_field(&message, |field, value| {
            seen.push((field, value));
            Some(())
        })
        .is_some()
    );
    assert_eq!(
        seen,
        [
            (1, Value::Varint(150)),
            (2, Value::Fixed64(0x0807_0605_0403_0201)),
            (4, Value::Fixed32(1))
        ]
    );
    for garbage in [
        &[0x0a, 0x05, 1][..],
        &[
            0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01,
        ],
        &[0x0e, 0x00],
        &[0x00, 0x00],
        &[0x1b, 0x08, 0x01],
        &[0x1c],
        &[0x25, 1, 2],
    ] {
        assert!(
            each_field(garbage, |_, _| Some(())).is_none(),
            "{garbage:?}"
        );
    }
    let mut out = Vec::new();
    assert!(repeated_fixed32(Value::Bytes(&[1, 2, 3]), &mut out).is_none());
    let mut values = Vec::new();
    assert!(repeated_varint(Value::Bytes(&[0x80]), &mut values).is_none());
}

#[test]
fn malformed_packets_never_panic() {
    assert!(decode(&[0; 15], &Keys::default()).is_none());
    let keys = Keys::new(&[LoraKey::MeshtasticChannel {
        name: "Open".to_owned(),
        psk: String::new(),
    }])
    .unwrap();
    let mut state = 0x9e37_79b9_u32;
    for length in 16..96 {
        for hash in [0x08, 0x1f, channel_hash("Open", &[]), 0] {
            let mut payload: Vec<u8> = (0..length)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    state.to_le_bytes()[0]
                })
                .collect();
            payload[13] = hash;
            let packet = decode(&payload, &keys).unwrap();
            assert_eq!(packet.channel_hash, hash);
            let _ = each_field(&payload, |_, _| Some(()));
        }
    }
}

#[test]
fn malformed_keys_are_reported() {
    let bad = LoraKey::MeshtasticChannel {
        name: "Bad".to_owned(),
        psk: "!!!".to_owned(),
    };
    assert!(Keys::new(&[bad]).is_err());
    let long = LoraKey::MeshtasticChannel {
        name: "Long".to_owned(),
        psk: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==".to_owned(),
    };
    assert!(Keys::new(&[long]).is_err());
    let other = LoraKey::LorawanAppKey {
        app_key: "zz".to_owned(),
    };
    assert!(Keys::new(&[other]).is_ok());
}

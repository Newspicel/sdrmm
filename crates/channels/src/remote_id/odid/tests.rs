use sdrmm_wire::{
    AuthType, EuCategory, EuClassification, HeightReference, OperatorLocationType,
    RemoteIdLocation, RemoteIdMessage, RemoteIdSystem, UaStatus, UaType, UasIdType,
};

use super::{Malformed, encode, parse, uas_id};
use crate::remote_id::ble::tests::{BT5_LONG_RANGE_PDU, bytes};

fn sniffed_pack() -> Vec<RemoteIdMessage> {
    let frame = bytes(BT5_LONG_RANGE_PDU);
    parse(&frame[18..frame.len() - 3]).unwrap()
}

#[test]
fn the_sniffed_pack_decodes_every_message() {
    let messages = sniffed_pack();
    let RemoteIdMessage::Location(location) = &messages[1] else {
        panic!("{messages:?}");
    };
    assert_eq!(location.status, UaStatus::Airborne);
    assert_eq!((location.lat, location.lon), (None, None));
    assert_eq!(location.track_deg, None);
    assert_eq!(location.speed_mps, None);
    assert_eq!(location.vertical_speed_mps, None);
    assert_eq!(location.pressure_altitude_m, Some(-55.0));
    assert_eq!(location.geodetic_altitude_m, None);
    assert_eq!(location.height_m, Some(-0.5));
    assert_eq!(location.pressure_accuracy_m, Some(3.0));
    assert_eq!(location.timestamp_accuracy_s, Some(0.1));
    assert_eq!(
        messages[2],
        RemoteIdMessage::SelfId {
            description_type: 0,
            text: "Drone ID demo".to_owned(),
        }
    );
    let RemoteIdMessage::System(system) = &messages[3] else {
        panic!("{messages:?}");
    };
    assert_eq!(system.operator_location_type, OperatorLocationType::Takeoff);
    assert_eq!(system.area_count, 1);
    assert_eq!(
        system.classification,
        Some(EuClassification {
            category: EuCategory::Open,
            class: Some(0),
        })
    );
    assert_eq!(
        messages[4],
        RemoteIdMessage::OperatorId {
            id_type: 0,
            operator_id: "FIN87astrdge12kxyz8".to_owned(),
        }
    );
    assert_eq!(uas_id(&messages).as_deref(), Some("SSEVTFG93700070"));
}

fn location() -> RemoteIdLocation {
    RemoteIdLocation {
        status: UaStatus::Airborne,
        lat: Some(47.397_658_5),
        lon: Some(8.545_603_4),
        track_deg: Some(271.0),
        speed_mps: Some(12.25),
        vertical_speed_mps: Some(-1.5),
        pressure_altitude_m: Some(512.5),
        geodetic_altitude_m: Some(530.0),
        height_m: Some(87.5),
        height_reference: HeightReference::Ground,
        horizontal_accuracy_m: Some(3.0),
        vertical_accuracy_m: Some(10.0),
        pressure_accuracy_m: Some(3.0),
        speed_accuracy_mps: Some(1.0),
        seconds_after_hour: Some(1_834.7),
        timestamp_accuracy_s: Some(0.2),
    }
}

fn every_message() -> Vec<RemoteIdMessage> {
    vec![
        RemoteIdMessage::BasicId {
            id_type: UasIdType::SerialNumber,
            ua_type: UaType::Rotorcraft,
            uas_id: "1596F3E4D5C6B7A80912".to_owned(),
        },
        RemoteIdMessage::BasicId {
            id_type: UasIdType::CaaRegistration,
            ua_type: UaType::Rotorcraft,
            uas_id: "FIN-OP-1234".to_owned(),
        },
        RemoteIdMessage::Location(location()),
        RemoteIdMessage::Authentication {
            auth_type: AuthType::MessageSetSignature,
            page: 0,
            last_page: Some(1),
            length: Some(40),
            timestamp: Some(1_700_000_000),
            data: "00112233445566778899aabbccddeeff10".to_owned(),
        },
        RemoteIdMessage::Authentication {
            auth_type: AuthType::MessageSetSignature,
            page: 1,
            last_page: None,
            length: None,
            timestamp: None,
            data: "0102030405060708090a0b0c0d0e0f1011121314151617".to_owned(),
        },
        RemoteIdMessage::SelfId {
            description_type: 1,
            text: "Lost link, returning".to_owned(),
        },
        RemoteIdMessage::System(RemoteIdSystem {
            operator_location_type: OperatorLocationType::LiveGnss,
            operator_lat: Some(47.396_1),
            operator_lon: Some(8.544_2),
            operator_altitude_m: Some(441.5),
            area_count: 3,
            area_radius_m: 250,
            area_ceiling_m: Some(120.0),
            area_floor_m: Some(10.0),
            classification: Some(EuClassification {
                category: EuCategory::Specific,
                class: Some(2),
            }),
            timestamp: Some(1_700_000_123),
        }),
        RemoteIdMessage::OperatorId {
            id_type: 0,
            operator_id: "FIN87astrdge12k8".to_owned(),
        },
    ]
}

#[test]
fn every_message_type_round_trips_through_a_pack() {
    let messages = every_message();
    let pack = encode::pack(&messages);
    assert_eq!(pack.len(), 3 + 25 * messages.len());
    assert_eq!(parse(&pack).unwrap(), messages);
}

#[test]
fn a_single_message_needs_no_pack() {
    let message = RemoteIdMessage::Location(location());
    assert_eq!(parse(&encode::message(&message)).unwrap(), vec![message]);
}

#[test]
fn unknown_values_decode_as_absent() {
    let mut raw = encode::message(&RemoteIdMessage::Location(location()));
    raw[1] |= 0x03;
    raw[2] = 181;
    raw[3] = 255;
    raw[4] = 126;
    raw[5..13].fill(0);
    raw[13..19].fill(0);
    raw[21..23].copy_from_slice(&0xFFFFu16.to_le_bytes());
    let [RemoteIdMessage::Location(decoded)] = &parse(&raw).unwrap()[..] else {
        panic!("not a location");
    };
    assert_eq!(decoded.track_deg, None);
    assert_eq!(decoded.speed_mps, None);
    assert_eq!(decoded.vertical_speed_mps, None);
    assert_eq!((decoded.lat, decoded.lon), (None, None));
    assert_eq!(decoded.geodetic_altitude_m, None);
    assert_eq!(decoded.seconds_after_hour, None);
}

#[test]
fn fast_drones_use_the_speed_multiplier() {
    let mut fast = location();
    fast.speed_mps = Some(100.5);
    let [RemoteIdMessage::Location(decoded)] =
        &parse(&encode::message(&RemoteIdMessage::Location(fast))).unwrap()[..]
    else {
        panic!("not a location");
    };
    assert_eq!(decoded.speed_mps, Some(100.5));
}

#[test]
fn malformed_packs_are_refused() {
    assert_eq!(parse(&[]), Err(Malformed::Short));
    assert_eq!(parse(&[0x12, 0x00]), Err(Malformed::Short));
    assert_eq!(parse(&[0xF2, 24, 1]), Err(Malformed::PackShape));
    assert_eq!(parse(&[0xF2, 25, 10]), Err(Malformed::PackShape));
    assert_eq!(parse(&[0xF2, 25, 2, 0x02]), Err(Malformed::Short));
}

#[test]
fn reserved_message_types_keep_their_bytes() {
    let mut raw = [0u8; 25];
    raw[0] = 0x72;
    raw[1] = 0xAB;
    let [RemoteIdMessage::Unknown { message_type, data }] = &parse(&raw).unwrap()[..] else {
        panic!("not unknown");
    };
    assert_eq!(*message_type, 7);
    assert!(data.starts_with("ab00"));
}

#[test]
fn a_serial_number_wins_over_a_session_id() {
    let messages = every_message();
    assert_eq!(uas_id(&messages[1..2]).as_deref(), Some("FIN-OP-1234"));
    assert_eq!(uas_id(&messages).as_deref(), Some("1596F3E4D5C6B7A80912"));
    assert_eq!(uas_id(&[]), None);
}

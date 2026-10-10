use sdrmm_wire::{
    AuthType, EuCategory, HeightReference, OperatorLocationType, RemoteIdLocation, RemoteIdMessage,
    RemoteIdSystem, UaStatus, UaType, UasIdType,
};

use super::{EPOCH_2019, ID_SIZE, MAX_PACK_MESSAGES, MESSAGE_SIZE, PACK, TEXT_SIZE};

const VERSION: u8 = 2;

pub fn message(message: &RemoteIdMessage) -> [u8; MESSAGE_SIZE] {
    let mut out = [0u8; MESSAGE_SIZE];
    match message {
        RemoteIdMessage::BasicId {
            id_type,
            ua_type,
            uas_id,
        } => {
            out[0] = header(0);
            out[1] = id_type_code(*id_type) << 4 | ua_type_code(*ua_type);
            put_text(&mut out[2..2 + ID_SIZE], uas_id);
        }
        RemoteIdMessage::Location(location) => put_location(&mut out, location),
        RemoteIdMessage::Authentication {
            auth_type,
            page,
            last_page,
            length,
            timestamp,
            data,
        } => {
            out[0] = header(2);
            out[1] = auth_type_code(*auth_type) << 4 | (page & 0x0F);
            let body = if *page == 0 {
                out[2] = last_page.unwrap_or(0);
                out[3] = length.unwrap_or(0);
                out[4..8].copy_from_slice(&to_2019(*timestamp).to_le_bytes());
                8
            } else {
                2
            };
            for (slot, byte) in out[body..].iter_mut().zip(unhex(data)) {
                *slot = byte;
            }
        }
        RemoteIdMessage::SelfId {
            description_type,
            text,
        } => {
            out[0] = header(3);
            out[1] = *description_type;
            put_text(&mut out[2..2 + TEXT_SIZE], text);
        }
        RemoteIdMessage::System(system) => put_system(&mut out, system),
        RemoteIdMessage::OperatorId {
            id_type,
            operator_id,
        } => {
            out[0] = header(5);
            out[1] = *id_type;
            put_text(&mut out[2..2 + ID_SIZE], operator_id);
        }
        RemoteIdMessage::Unknown { message_type, data } => {
            out[0] = header(*message_type);
            for (slot, byte) in out[1..].iter_mut().zip(unhex(data)) {
                *slot = byte;
            }
        }
    }
    out
}

pub fn pack(messages: &[RemoteIdMessage]) -> Vec<u8> {
    let count = messages.len().min(MAX_PACK_MESSAGES);
    let mut out = vec![header(PACK), MESSAGE_SIZE as u8, count as u8];
    for entry in &messages[..count] {
        out.extend_from_slice(&message(entry));
    }
    out
}

fn header(message_type: u8) -> u8 {
    message_type << 4 | VERSION
}

fn put_location(out: &mut [u8; MESSAGE_SIZE], location: &RemoteIdLocation) {
    let track = location.track_deg.unwrap_or(361.0).round() as u16;
    let (speed, multiplied) = speed_code(location.speed_mps);
    out[0] = header(1);
    out[1] = status_code(location.status) << 4
        | u8::from(location.height_reference == HeightReference::Ground) << 2
        | u8::from(track >= 180) << 1
        | u8::from(multiplied);
    out[2] = (track % 180) as u8;
    out[3] = speed;
    out[4] = location.vertical_speed_mps.map_or(126, |speed| {
        (speed / 0.5).round().clamp(-124.0, 124.0) as i8
    }) as u8;
    out[5..9].copy_from_slice(&coordinate(location.lat).to_le_bytes());
    out[9..13].copy_from_slice(&coordinate(location.lon).to_le_bytes());
    out[13..15].copy_from_slice(&altitude(location.pressure_altitude_m).to_le_bytes());
    out[15..17].copy_from_slice(&altitude(location.geodetic_altitude_m).to_le_bytes());
    out[17..19].copy_from_slice(&altitude(location.height_m).to_le_bytes());
    out[19] = accuracy_code(&super::VERTICAL_ACCURACY_M, location.vertical_accuracy_m) << 4
        | accuracy_code(
            &super::HORIZONTAL_ACCURACY_M,
            location.horizontal_accuracy_m,
        );
    out[20] = accuracy_code(&super::VERTICAL_ACCURACY_M, location.pressure_accuracy_m) << 4
        | accuracy_code(&super::SPEED_ACCURACY_MPS, location.speed_accuracy_mps);
    let timestamp = location
        .seconds_after_hour
        .map_or(0xFFFF, |seconds| (seconds * 10.0).round() as u16);
    out[21..23].copy_from_slice(&timestamp.to_le_bytes());
    out[23] = location
        .timestamp_accuracy_s
        .map_or(0, |seconds| (seconds * 10.0).round().clamp(0.0, 15.0) as u8);
}

fn put_system(out: &mut [u8; MESSAGE_SIZE], system: &RemoteIdSystem) {
    out[0] = header(4);
    out[1] = u8::from(system.classification.is_some()) << 2
        | match system.operator_location_type {
            OperatorLocationType::Takeoff => 0,
            OperatorLocationType::LiveGnss => 1,
            OperatorLocationType::Fixed => 2,
            OperatorLocationType::Reserved => 3,
        };
    out[2..6].copy_from_slice(&coordinate(system.operator_lat).to_le_bytes());
    out[6..10].copy_from_slice(&coordinate(system.operator_lon).to_le_bytes());
    out[10..12].copy_from_slice(&system.area_count.to_le_bytes());
    out[12] = (system.area_radius_m / 10).min(255) as u8;
    out[13..15].copy_from_slice(&altitude(system.area_ceiling_m).to_le_bytes());
    out[15..17].copy_from_slice(&altitude(system.area_floor_m).to_le_bytes());
    out[17] = system.classification.map_or(0, |classification| {
        let category = match classification.category {
            EuCategory::Undeclared => 0,
            EuCategory::Open => 1,
            EuCategory::Specific => 2,
            EuCategory::Certified => 3,
            EuCategory::Reserved => 4,
        };
        category << 4 | classification.class.map_or(0, |class| class + 1)
    });
    out[18..20].copy_from_slice(&altitude(system.operator_altitude_m).to_le_bytes());
    out[20..24].copy_from_slice(&to_2019(system.timestamp).to_le_bytes());
}

fn speed_code(speed: Option<f32>) -> (u8, bool) {
    match speed {
        None => (255, true),
        Some(speed) if speed <= 63.75 => ((speed / 0.25).round() as u8, false),
        Some(speed) => (((speed - 63.75) / 0.75).round().min(254.0) as u8, true),
    }
}

fn coordinate(degrees: Option<f64>) -> i32 {
    degrees.map_or(0, |degrees| (degrees * 1e7).round() as i32)
}

fn altitude(metres: Option<f32>) -> u16 {
    metres.map_or(0, |metres| ((metres + 1_000.0) / 0.5).round() as u16)
}

fn accuracy_code(table: &[f32], value: Option<f32>) -> u8 {
    value
        .and_then(|value| table.iter().position(|&bound| bound <= value))
        .map_or(0, |index| index as u8 + 1)
}

fn to_2019(timestamp: Option<i64>) -> u32 {
    timestamp.map_or(0, |unix| (unix - EPOCH_2019).max(0) as u32)
}

fn put_text(out: &mut [u8], text: &str) {
    for (slot, byte) in out.iter_mut().zip(text.bytes()) {
        *slot = byte;
    }
}

fn unhex(text: &str) -> impl Iterator<Item = u8> + '_ {
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .filter_map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
}

fn status_code(status: UaStatus) -> u8 {
    match status {
        UaStatus::Undeclared => 0,
        UaStatus::Ground => 1,
        UaStatus::Airborne => 2,
        UaStatus::Emergency => 3,
        UaStatus::SystemFailure => 4,
        UaStatus::Reserved => 5,
    }
}

fn id_type_code(id_type: UasIdType) -> u8 {
    match id_type {
        UasIdType::None => 0,
        UasIdType::SerialNumber => 1,
        UasIdType::CaaRegistration => 2,
        UasIdType::UtmAssigned => 3,
        UasIdType::SpecificSession => 4,
        UasIdType::FrenchIdentifier | UasIdType::Reserved => 5,
    }
}

fn ua_type_code(ua_type: UaType) -> u8 {
    (0..16)
        .find(|&code| super::ua_type(code) == ua_type)
        .unwrap_or(15)
}

fn auth_type_code(auth_type: AuthType) -> u8 {
    match auth_type {
        AuthType::None => 0,
        AuthType::UasIdSignature => 1,
        AuthType::OperatorIdSignature => 2,
        AuthType::MessageSetSignature => 3,
        AuthType::NetworkRemoteId => 4,
        AuthType::SpecificMethod => 5,
        AuthType::Reserved => 6,
        AuthType::Private => 0x0A,
    }
}

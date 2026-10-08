use sdrmm_wire::{
    AuthType, EuCategory, EuClassification, HeightReference, OperatorLocationType,
    RemoteIdLocation, RemoteIdMessage, RemoteIdSystem, UaStatus, UaType, UasIdType,
};

use crate::datalink::hex;

pub(crate) const MESSAGE_SIZE: usize = 25;
pub(crate) const MAX_PACK_MESSAGES: usize = 9;
pub(crate) const APP_CODE: u8 = 0x0D;
pub(crate) const SERVICE_UUID: u16 = 0xFFFA;
const PACK: u8 = 0xF;
const ID_SIZE: usize = 20;
const TEXT_SIZE: usize = 23;
const EPOCH_2019: i64 = 1_546_300_800;
const INVALID_TIMESTAMP: u16 = 0xFFFF;
const INVALID_SPEED: u8 = 255;
const INVALID_VERTICAL_SPEED: i8 = 126;
const MAX_DIRECTION_DEG: f32 = 360.0;
const SECONDS_PER_HOUR: f32 = 3_600.0;
const HORIZONTAL_ACCURACY_M: [f32; 12] = [
    18_520.0, 7_408.0, 3_704.0, 1_852.0, 926.0, 555.6, 185.2, 92.6, 30.0, 10.0, 3.0, 1.0,
];
const VERTICAL_ACCURACY_M: [f32; 6] = [150.0, 45.0, 25.0, 10.0, 3.0, 1.0];
const SPEED_ACCURACY_MPS: [f32; 4] = [10.0, 3.0, 1.0, 0.3];

#[derive(Debug, PartialEq)]
pub(crate) enum Malformed {
    Short,
    PackShape,
}

pub(crate) fn parse(data: &[u8]) -> Result<Vec<RemoteIdMessage>, Malformed> {
    let header = *data.first().ok_or(Malformed::Short)?;
    if header >> 4 != PACK {
        return single(data).map(|message| vec![message]);
    }
    let [_, size, count, body @ ..] = data else {
        return Err(Malformed::Short);
    };
    let (size, count) = (usize::from(*size), usize::from(*count));
    if size != MESSAGE_SIZE || count > MAX_PACK_MESSAGES {
        return Err(Malformed::PackShape);
    }
    if body.len() < count * MESSAGE_SIZE {
        return Err(Malformed::Short);
    }
    body.as_chunks::<MESSAGE_SIZE>()
        .0
        .iter()
        .take(count)
        .filter(|message| message[0] >> 4 != PACK)
        .map(|message| single(message))
        .collect()
}

fn single(data: &[u8]) -> Result<RemoteIdMessage, Malformed> {
    let message: &[u8; MESSAGE_SIZE] = data
        .get(..MESSAGE_SIZE)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(Malformed::Short)?;
    Ok(match message[0] >> 4 {
        0 => basic_id(message),
        1 => RemoteIdMessage::Location(location(message)),
        2 => authentication(message),
        3 => RemoteIdMessage::SelfId {
            description_type: message[1],
            text: text(&message[2..2 + TEXT_SIZE]),
        },
        4 => RemoteIdMessage::System(system(message)),
        5 => RemoteIdMessage::OperatorId {
            id_type: message[1],
            operator_id: text(&message[2..2 + ID_SIZE]),
        },
        other => RemoteIdMessage::Unknown {
            message_type: other,
            data: hex(&message[1..]),
        },
    })
}

fn basic_id(message: &[u8; MESSAGE_SIZE]) -> RemoteIdMessage {
    RemoteIdMessage::BasicId {
        id_type: match message[1] >> 4 {
            0 => UasIdType::None,
            1 => UasIdType::SerialNumber,
            2 => UasIdType::CaaRegistration,
            3 => UasIdType::UtmAssigned,
            4 => UasIdType::SpecificSession,
            _ => UasIdType::Reserved,
        },
        ua_type: ua_type(message[1] & 0x0F),
        uas_id: text(&message[2..2 + ID_SIZE]),
    }
}

fn ua_type(code: u8) -> UaType {
    match code {
        0 => UaType::None,
        1 => UaType::Aeroplane,
        2 => UaType::Rotorcraft,
        3 => UaType::Gyroplane,
        4 => UaType::HybridLift,
        5 => UaType::Ornithopter,
        6 => UaType::Glider,
        7 => UaType::Kite,
        8 => UaType::FreeBalloon,
        9 => UaType::CaptiveBalloon,
        10 => UaType::Airship,
        11 => UaType::Parachute,
        12 => UaType::Rocket,
        13 => UaType::TetheredAircraft,
        14 => UaType::GroundObstacle,
        _ => UaType::Other,
    }
}

fn location(message: &[u8; MESSAGE_SIZE]) -> RemoteIdLocation {
    let flags = message[1];
    let direction = f32::from(message[2]) + if flags & 0x02 != 0 { 180.0 } else { 0.0 };
    let speed = message[3];
    let vertical = message[4] as i8;
    let timestamp = u16_at(message, 21);
    RemoteIdLocation {
        status: match flags >> 4 {
            0 => UaStatus::Undeclared,
            1 => UaStatus::Ground,
            2 => UaStatus::Airborne,
            3 => UaStatus::Emergency,
            4 => UaStatus::SystemFailure,
            _ => UaStatus::Reserved,
        },
        lat: coordinate(i32_at(message, 5), 90.0),
        lon: coordinate(i32_at(message, 9), 180.0),
        track_deg: (direction < MAX_DIRECTION_DEG).then_some(direction),
        speed_mps: horizontal_speed(speed, flags & 0x01 != 0),
        vertical_speed_mps: (vertical != INVALID_VERTICAL_SPEED).then(|| f32::from(vertical) * 0.5),
        pressure_altitude_m: altitude(u16_at(message, 13)),
        geodetic_altitude_m: altitude(u16_at(message, 15)),
        height_m: altitude(u16_at(message, 17)),
        height_reference: if flags & 0x04 != 0 {
            HeightReference::Ground
        } else {
            HeightReference::Takeoff
        },
        horizontal_accuracy_m: accuracy(&HORIZONTAL_ACCURACY_M, message[19] & 0x0F),
        vertical_accuracy_m: accuracy(&VERTICAL_ACCURACY_M, message[19] >> 4),
        pressure_accuracy_m: accuracy(&VERTICAL_ACCURACY_M, message[20] >> 4),
        speed_accuracy_mps: accuracy(&SPEED_ACCURACY_MPS, message[20] & 0x0F),
        seconds_after_hour: (timestamp != INVALID_TIMESTAMP)
            .then(|| f32::from(timestamp) / 10.0)
            .filter(|&seconds| seconds <= SECONDS_PER_HOUR),
        timestamp_accuracy_s: (message[23] & 0x0F != 0)
            .then(|| f32::from(message[23] & 0x0F) / 10.0),
    }
}

fn horizontal_speed(encoded: u8, multiplied: bool) -> Option<f32> {
    match (encoded, multiplied) {
        (INVALID_SPEED, true) => None,
        (value, true) => Some(f32::from(value) * 0.75 + 255.0 * 0.25),
        (value, false) => Some(f32::from(value) * 0.25),
    }
}

fn authentication(message: &[u8; MESSAGE_SIZE]) -> RemoteIdMessage {
    let page = message[1] & 0x0F;
    let auth_type = match message[1] >> 4 {
        0 => AuthType::None,
        1 => AuthType::UasIdSignature,
        2 => AuthType::OperatorIdSignature,
        3 => AuthType::MessageSetSignature,
        4 => AuthType::NetworkRemoteId,
        5 => AuthType::SpecificMethod,
        6..=9 => AuthType::Reserved,
        _ => AuthType::Private,
    };
    if page == 0 {
        RemoteIdMessage::Authentication {
            auth_type,
            page,
            last_page: Some(message[2]),
            length: Some(message[3]),
            timestamp: since_2019(u32_at(message, 4)),
            data: hex(&message[8..]),
        }
    } else {
        RemoteIdMessage::Authentication {
            auth_type,
            page,
            last_page: None,
            length: None,
            timestamp: None,
            data: hex(&message[2..]),
        }
    }
}

fn system(message: &[u8; MESSAGE_SIZE]) -> RemoteIdSystem {
    let flags = message[1];
    let classified = (flags >> 2) & 0x07 == 1;
    RemoteIdSystem {
        operator_location_type: match flags & 0x03 {
            0 => OperatorLocationType::Takeoff,
            1 => OperatorLocationType::LiveGnss,
            2 => OperatorLocationType::Fixed,
            _ => OperatorLocationType::Reserved,
        },
        operator_lat: coordinate(i32_at(message, 2), 90.0),
        operator_lon: coordinate(i32_at(message, 6), 180.0),
        area_count: u16_at(message, 10),
        area_radius_m: u16::from(message[12]) * 10,
        area_ceiling_m: altitude(u16_at(message, 13)),
        area_floor_m: altitude(u16_at(message, 15)),
        classification: classified.then(|| EuClassification {
            category: match message[17] >> 4 {
                0 => EuCategory::Undeclared,
                1 => EuCategory::Open,
                2 => EuCategory::Specific,
                3 => EuCategory::Certified,
                _ => EuCategory::Reserved,
            },
            class: match message[17] & 0x0F {
                class @ 1..=7 => Some(class - 1),
                _ => None,
            },
        }),
        operator_altitude_m: altitude(u16_at(message, 18)),
        timestamp: since_2019(u32_at(message, 20)),
    }
}

fn coordinate(encoded: i32, limit: f64) -> Option<f64> {
    let degrees = f64::from(encoded) / 1e7;
    (encoded != 0 && degrees.abs() <= limit).then_some(degrees)
}

fn altitude(encoded: u16) -> Option<f32> {
    (encoded != 0).then(|| f32::from(encoded) * 0.5 - 1_000.0)
}

fn accuracy(table: &[f32], code: u8) -> Option<f32> {
    usize::from(code)
        .checked_sub(1)
        .and_then(|index| table.get(index))
        .copied()
}

fn since_2019(seconds: u32) -> Option<i64> {
    (seconds != 0).then(|| EPOCH_2019 + i64::from(seconds))
}

fn text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    bytes[..end]
        .iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                char::from(b)
            } else {
                '?'
            }
        })
        .collect::<String>()
        .trim_end()
        .to_owned()
}

fn u16_at(message: &[u8; MESSAGE_SIZE], at: usize) -> u16 {
    u16::from_le_bytes([message[at], message[at + 1]])
}

fn u32_at(message: &[u8; MESSAGE_SIZE], at: usize) -> u32 {
    u32::from_le_bytes([
        message[at],
        message[at + 1],
        message[at + 2],
        message[at + 3],
    ])
}

fn i32_at(message: &[u8; MESSAGE_SIZE], at: usize) -> i32 {
    u32_at(message, at) as i32
}

pub(crate) fn uas_id(messages: &[RemoteIdMessage]) -> Option<String> {
    let ids = messages.iter().filter_map(|message| match message {
        RemoteIdMessage::BasicId {
            id_type, uas_id, ..
        } if !uas_id.is_empty() => Some((*id_type, uas_id)),
        _ => None,
    });
    ids.clone()
        .find(|(id_type, _)| *id_type == UasIdType::SerialNumber)
        .or_else(|| ids.clone().next())
        .map(|(_, id)| id.clone())
}

#[cfg(any(test, feature = "synth"))]
pub mod encode;

#[cfg(test)]
mod tests;

//! The BSS frame: a `0x01` marker, then length-type-value fields up to the FCS. A length
//! counts the type byte; the length `0x85` instead carries a two-byte message ID.

use super::BssPacket;
use crate::datalink::hex;

pub(super) const MARKER: u8 = 0x01;
const MESSAGE_ID: u8 = 0x85;

const SOURCE: u8 = 0x20;
const DESTINATION: u8 = 0x21;
const MESSAGE: u8 = 0x24;
const LOCATION: u8 = 0x25;
const LOCATION_REQUEST: u8 = 0x27;
const CALL_REQUEST: u8 = 0x28;

/// Latitude and longitude count 1/500 of an arc minute.
const UNITS_PER_DEGREE: f64 = 60.0 * 500.0;
const NO_ALTITUDE: i16 = i16::MIN;
const NOT_KNOWN: u16 = u16::MAX;
const FIX_BYTES: usize = 6;
const MOTION_BYTES: usize = 12;

enum Field<'a> {
    MessageId(u16),
    Value(u8, &'a [u8]),
}

/// Hands each field of `body`, the frame without its FCS, to `each`. `None` when the fields do
/// not end exactly where the frame does, or `each` refuses one.
fn walk<'a>(body: &'a [u8], mut each: impl FnMut(Field<'a>) -> Option<()>) -> Option<()> {
    let (&marker, mut rest) = body.split_first()?;
    if marker != MARKER || rest.is_empty() {
        return None;
    }
    while let Some((&length, tail)) = rest.split_first() {
        if length == MESSAGE_ID {
            let (id, tail) = tail.split_first_chunk::<2>()?;
            each(Field::MessageId(u16::from_be_bytes(*id)))?;
            rest = tail;
            continue;
        }
        let field = tail.get(..usize::from(length))?;
        let (&kind, value) = field.split_first()?;
        each(Field::Value(kind, value))?;
        rest = &tail[field.len()..];
    }
    Some(())
}

/// Whether `body` is laid out as BSS fields, without reading them.
pub(super) fn lines_up(body: &[u8]) -> bool {
    walk(body, |_| Some(())).is_some()
}

pub(super) fn parse(body: &[u8]) -> Option<BssPacket> {
    let mut packet = BssPacket {
        frame: hex(body),
        ..BssPacket::default()
    };
    walk(body, |field| match field {
        Field::MessageId(id) => {
            packet.message_id = Some(id);
            Some(())
        }
        Field::Value(kind, value) => apply(&mut packet, kind, value),
    })?;
    Some(packet)
}

fn apply(packet: &mut BssPacket, kind: u8, value: &[u8]) -> Option<()> {
    match kind {
        SOURCE => packet.source = callsign(value).unwrap_or_default(),
        DESTINATION => packet.destination = callsign(value),
        MESSAGE => packet.message = Some(String::from_utf8_lossy(value).into_owned()),
        LOCATION => location(packet, value)?,
        LOCATION_REQUEST => packet.location_request = callsign(value),
        CALL_REQUEST => packet.call_request = callsign(value),
        _ => packet.unknown.push(format!("{kind:02x}:{}", hex(value))),
    }
    Some(())
}

fn callsign(value: &[u8]) -> Option<String> {
    let call = String::from_utf8_lossy(value).trim().to_owned();
    (!call.is_empty()).then_some(call)
}

fn location(packet: &mut BssPacket, value: &[u8]) -> Option<()> {
    let fix = value.first_chunk::<FIX_BYTES>()?;
    let lat = degrees([fix[0], fix[1], fix[2]]);
    let lon = degrees([fix[3], fix[4], fix[5]]);
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    (packet.lat, packet.lon) = (Some(lat), Some(lon));
    if let Some(motion) = value.first_chunk::<MOTION_BYTES>() {
        let altitude = i16::from_be_bytes([motion[6], motion[7]]);
        let word = |at: usize| u16::from_be_bytes([motion[at], motion[at + 1]]);
        packet.altitude_m = (altitude != NO_ALTITUDE).then_some(altitude);
        packet.speed_kmh = Some(word(8)).filter(|&v| v != NOT_KNOWN);
        packet.heading_deg = Some(word(10)).filter(|&v| v != NOT_KNOWN);
    }
    Some(())
}

fn degrees(be24: [u8; 3]) -> f64 {
    let raw = i32::from_be_bytes([be24[0], be24[1], be24[2], 0]) >> 8;
    f64::from(raw) / UNITS_PER_DEGREE
}

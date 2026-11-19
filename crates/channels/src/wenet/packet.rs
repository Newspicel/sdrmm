//! The 256-byte Wenet packets: the first byte names the type.

use jiff::Timestamp;

use super::model::{WenetMode, WenetPacket, WenetTelemetry};

pub(crate) const TEXT: u8 = 0x00;
pub(crate) const GPS: u8 = 0x01;
pub(crate) const IMAGE_TELEMETRY: u8 = 0x54;
pub(crate) const SSDV: u8 = 0x55;
pub(crate) const IDLE: u8 = 0x56;

const GPS_EPOCH_UNIX: i64 = 315_964_800;
const SECONDS_PER_WEEK: i64 = 604_800;
const UNUSED: u8 = 0x55;
const GPS_FIX_AT: usize = 1;
const IMAGE_FIX_AT: usize = 11;
const IMAGE_CALLSIGN_AT: usize = 3;
const IMAGE_CALLSIGN_LEN: usize = 6;
const SYSTEM_AT: usize = 35;
const SYSTEM_END: usize = 73;

/// What a packet carries.
pub(crate) enum Payload {
    Telemetry(WenetTelemetry),
    Ssdv,
    Idle,
    Other,
}

pub(crate) fn parse(packet: &[u8], mode: WenetMode) -> Payload {
    match packet.first().copied() {
        Some(TEXT) => text(packet, mode).map_or(Payload::Other, Payload::Telemetry),
        Some(GPS) => gps(packet, mode).map_or(Payload::Other, Payload::Telemetry),
        Some(IMAGE_TELEMETRY) => {
            image_telemetry(packet, mode).map_or(Payload::Other, Payload::Telemetry)
        }
        Some(SSDV) => Payload::Ssdv,
        Some(IDLE) => Payload::Idle,
        _ => Payload::Other,
    }
}

fn empty(mode: WenetMode, packet: WenetPacket) -> WenetTelemetry {
    WenetTelemetry {
        mode,
        packet,
        callsign: None,
        message_id: None,
        text: None,
        image_id: None,
        time: None,
        lat: None,
        lon: None,
        altitude_m: None,
        speed_kmh: None,
        heading_deg: None,
        climb_ms: None,
        satellites: None,
        cpu_temp_c: None,
        radio_temp_c: None,
    }
}

fn text(packet: &[u8], mode: WenetMode) -> Option<WenetTelemetry> {
    let len = usize::from(*packet.get(1)?);
    let id = u16::from_be_bytes([*packet.get(2)?, *packet.get(3)?]);
    let body = packet.get(4..4 + len)?;
    let message: String = body
        .iter()
        .map(|&byte| {
            if byte.is_ascii_graphic() || byte == b' ' {
                char::from(byte)
            } else {
                ' '
            }
        })
        .collect();
    Some(WenetTelemetry {
        message_id: Some(id),
        text: Some(message.trim().to_owned()),
        ..empty(mode, WenetPacket::Text)
    })
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn u8(&self, at: usize) -> Option<u8> {
        self.0.get(at).copied()
    }

    fn u16(&self, at: usize) -> Option<u16> {
        Some(u16::from_be_bytes(self.0.get(at..at + 2)?.try_into().ok()?))
    }

    fn u32(&self, at: usize) -> Option<u32> {
        Some(u32::from_be_bytes(self.0.get(at..at + 4)?.try_into().ok()?))
    }

    fn f32(&self, at: usize) -> Option<f32> {
        Some(f32::from_bits(self.u32(at)?)).filter(|value| value.is_finite())
    }
}

/// Week, time of week, leap seconds, position, motion and satellites, as GPS and image
/// telemetry packets both lay them out from `at`.
fn fix(reader: &Reader<'_>, at: usize, into: &mut WenetTelemetry) -> Option<()> {
    let week = i64::from(reader.u16(at)?);
    let tow_ms = i64::from(reader.u32(at + 2)?);
    let leap = i64::from(reader.u8(at + 6)?);
    let unix = GPS_EPOCH_UNIX + week * SECONDS_PER_WEEK + tow_ms / 1_000 - leap;
    let satellites = reader.u8(at + 31)?;
    let has_fix = reader.u8(at + 32)? > 0;
    into.time = Timestamp::from_second(unix)
        .ok()
        .map(|stamp| stamp.strftime("%Y-%m-%dT%H:%M:%SZ").to_string());
    into.satellites = Some(satellites);
    if has_fix {
        into.lat = reader.f32(at + 7).map(f64::from);
        into.lon = reader.f32(at + 11).map(f64::from);
        into.altitude_m = reader.f32(at + 15).map(f64::from);
        into.speed_kmh = reader.f32(at + 19);
        into.heading_deg = reader.f32(at + 23);
        into.climb_ms = reader.f32(at + 27);
    }
    Some(())
}

fn gps(packet: &[u8], mode: WenetMode) -> Option<WenetTelemetry> {
    let reader = Reader(packet);
    let mut telemetry = empty(mode, WenetPacket::Gps);
    fix(&reader, GPS_FIX_AT, &mut telemetry)?;
    let system = packet.get(SYSTEM_AT..SYSTEM_END)?;
    if system.iter().any(|&byte| byte != UNUSED) {
        telemetry.radio_temp_c = reader.f32(SYSTEM_AT);
        telemetry.cpu_temp_c = reader.f32(SYSTEM_AT + 4);
    }
    Some(telemetry)
}

fn image_telemetry(packet: &[u8], mode: WenetMode) -> Option<WenetTelemetry> {
    let reader = Reader(packet);
    let mut telemetry = empty(mode, WenetPacket::ImageTelemetry);
    let len = usize::from(reader.u8(IMAGE_CALLSIGN_AT)?).min(IMAGE_CALLSIGN_LEN);
    let callsign = packet.get(IMAGE_CALLSIGN_AT + 1..IMAGE_CALLSIGN_AT + 1 + len)?;
    telemetry.callsign = std::str::from_utf8(callsign)
        .ok()
        .map(str::trim)
        .filter(|call| !call.is_empty())
        .map(str::to_owned);
    telemetry.image_id = reader.u8(IMAGE_CALLSIGN_AT + 1 + IMAGE_CALLSIGN_LEN);
    fix(&reader, IMAGE_FIX_AT, &mut telemetry)?;
    Some(telemetry)
}

/// Packet builders for tests and the modulator.
#[cfg(any(test, feature = "synth"))]
pub(crate) mod build {
    use super::{GPS, PACKET_FILL, TEXT};
    use crate::wenet::frame::PACKET_LEN;

    pub(crate) fn text(id: u16, message: &str) -> [u8; PACKET_LEN] {
        let mut packet = [PACKET_FILL; PACKET_LEN];
        let body = &message.as_bytes()[..message.len().min(PACKET_LEN - 4)];
        packet[0] = TEXT;
        packet[1] = body.len() as u8;
        packet[2..4].copy_from_slice(&id.to_be_bytes());
        packet[4..4 + body.len()].copy_from_slice(body);
        packet
    }

    /// A GPS packet with the base fields only.
    pub(crate) fn gps(
        week: u16,
        tow_ms: u32,
        lat: f32,
        lon: f32,
        alt: f32,
        climb: f32,
    ) -> [u8; PACKET_LEN] {
        let mut packet = [PACKET_FILL; PACKET_LEN];
        let mut at = 0;
        let mut put = |bytes: &[u8]| {
            packet[at..at + bytes.len()].copy_from_slice(bytes);
            at += bytes.len();
        };
        put(&[GPS]);
        put(&week.to_be_bytes());
        put(&tow_ms.to_be_bytes());
        put(&[18]);
        for value in [lat, lon, alt, 30.0, 270.0, climb] {
            put(&value.to_be_bytes());
        }
        put(&[11, 3, 6]);
        packet
    }
}

#[cfg(any(test, feature = "synth"))]
const PACKET_FILL: u8 = UNUSED;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_messages_read_back() {
        let Payload::Telemetry(event) = parse(&build::text(42, "Wenet test"), WenetMode::Uart)
        else {
            panic!("telemetry");
        };
        assert_eq!(event.text.as_deref(), Some("Wenet test"));
        assert_eq!(event.message_id, Some(42));
    }

    #[test]
    fn gps_packets_carry_a_fix_and_utc_time() {
        let packet = build::gps(2_400, 345_600_000, -34.95, 138.52, 25_000.0, 5.5);
        let Payload::Telemetry(event) = parse(&packet, WenetMode::I2s) else {
            panic!("telemetry");
        };
        assert_eq!(event.packet, WenetPacket::Gps);
        assert!((event.lat.expect("lat") + 34.95).abs() < 1e-5);
        assert_eq!(event.altitude_m, Some(25_000.0));
        assert_eq!(event.satellites, Some(11));
        assert_eq!(event.radio_temp_c, None);
        assert_eq!(event.time.as_deref(), Some("2026-01-07T23:59:42Z"));
    }

    #[test]
    fn ssdv_and_idle_are_told_apart() {
        assert!(matches!(
            parse(&[SSDV, 0x67], WenetMode::Uart),
            Payload::Ssdv
        ));
        assert!(matches!(parse(&[IDLE; 4], WenetMode::Uart), Payload::Idle));
        assert!(matches!(parse(&[0x02; 4], WenetMode::Uart), Payload::Other));
    }
}

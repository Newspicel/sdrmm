//! The payloads layer 2 carries: the fixed v1 and v2 structs, and the CRC that tells which
//! format a frame is.

use super::layer2::crc;
use super::model::HorusFrame;
use super::v3;

const V1_BYTES: usize = 22;
const V2_BYTES: usize = 32;
const CRC_BYTES: usize = 2;
const BATTERY_FULL_SCALE_V: f32 = 5.0;
const CUSTOM_BYTES: usize = 9;

/// The telemetry `payload` holds, or `None` when its CRC fails or v3 content will not parse.
pub(crate) fn parse(payload: &[u8]) -> Option<HorusFrame> {
    let tail_ok = |len| payload.len() == len && crc_at_end(payload);
    if tail_ok(V1_BYTES) {
        return Some(fixed(payload, Fixed::V1));
    }
    if tail_ok(V2_BYTES) {
        return Some(fixed(payload, Fixed::V2));
    }
    let (check, body) = payload.split_first_chunk::<CRC_BYTES>()?;
    (payload.len() > V1_BYTES && u16::from_le_bytes(*check) == crc(body))
        .then(|| v3::parse(body))
        .flatten()
}

fn crc_at_end(payload: &[u8]) -> bool {
    payload
        .split_last_chunk::<CRC_BYTES>()
        .is_some_and(|(body, check)| crc(body) == u16::from_le_bytes(*check))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fixed {
    V1,
    V2,
}

/// Reads the little-endian struct v1 and v2 share after the payload ID.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0; N];
        if let Some(bytes) = self.bytes.get(self.at..self.at + N) {
            out.copy_from_slice(bytes);
        }
        self.at += N;
        out
    }

    fn u8(&mut self) -> u8 {
        self.take::<1>()[0]
    }

    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take())
    }

    fn f32(&mut self) -> f32 {
        f32::from_le_bytes(self.take())
    }
}

fn fixed(payload: &[u8], format: Fixed) -> HorusFrame {
    let mut read = Reader {
        bytes: payload,
        at: 0,
    };
    let id = match format {
        Fixed::V1 => u16::from(read.u8()),
        Fixed::V2 => read.u16(),
    };
    let sequence = read.u16();
    let [hours, minutes, seconds] = read.take::<3>();
    let version = if format == Fixed::V1 { 1 } else { 2 };
    let mut frame = HorusFrame::new(version, id.to_string(), sequence);
    frame.time = time_of_day(hours, minutes, seconds);
    let (lat, lon) = (read.f32(), read.f32());
    frame.place(f64::from(lat), f64::from(lon));
    frame.altitude_m = Some(f64::from(read.u16()));
    frame.speed_kmh = Some(f64::from(read.u8()));
    frame.satellites = Some(read.u8());
    frame.temperature_c = Some(f32::from(read.u8() as i8));
    frame.battery_v = Some(f32::from(read.u8()) * BATTERY_FULL_SCALE_V / 255.0);
    if format == Fixed::V2 {
        frame.custom = Some(hex(&read.take::<CUSTOM_BYTES>()));
    }
    frame
}

fn time_of_day(hours: u8, minutes: u8, seconds: u8) -> Option<String> {
    (hours < 24 && minutes < 60 && seconds < 60)
        .then(|| format!("{hours:02}:{minutes:02}:{seconds:02}"))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

#[cfg(test)]
pub(crate) fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_v1_frame_from_the_iq_recording() {
        let frame = parse(&unhex("000900071E2A000000000000000000000000259A6B14")).expect("v1");
        assert_eq!(frame.version, 1);
        assert_eq!(frame.payload, "0");
        assert_eq!(frame.sequence, 9);
        assert_eq!(frame.time.as_deref(), Some("07:30:42"));
        assert_eq!((frame.lat, frame.lon), (None, None));
        assert_eq!(frame.altitude_m, Some(0.0));
        assert_eq!(frame.satellites, Some(0));
        assert_eq!(frame.temperature_c, Some(37.0));
        let battery = frame.battery_v.expect("battery");
        assert!((battery - 3.02).abs() < 0.01, "{battery}");
    }

    #[test]
    fn reads_a_v2_frame_with_a_position() {
        let mut payload = vec![0x01, 0x01, 0x05, 0x00, 12, 34, 56];
        payload.extend((-34.9f32).to_le_bytes());
        payload.extend(138.6f32.to_le_bytes());
        payload.extend(12_345u16.to_le_bytes());
        payload.extend([42, 9, (-20i8) as u8, 255]);
        payload.extend(1..=9u8);
        payload.extend(crc(&payload).to_le_bytes());
        let frame = parse(&payload).expect("v2");
        assert_eq!(frame.version, 2);
        assert_eq!(frame.payload, "257");
        assert_eq!(frame.sequence, 5);
        assert_eq!(frame.time.as_deref(), Some("12:34:56"));
        let lat = frame.lat.expect("lat");
        assert!((lat + 34.9).abs() < 1e-5, "{lat}");
        assert_eq!(frame.altitude_m, Some(12_345.0));
        assert_eq!(frame.speed_kmh, Some(42.0));
        assert_eq!(frame.temperature_c, Some(-20.0));
        assert_eq!(frame.battery_v, Some(5.0));
        assert_eq!(frame.custom.as_deref(), Some("010203040506070809"));
    }

    #[test]
    fn a_bad_crc_is_refused() {
        let mut payload = unhex("000900071E2A000000000000000000000000259A6B14");
        payload[3] ^= 1;
        assert!(parse(&payload).is_none());
    }
}

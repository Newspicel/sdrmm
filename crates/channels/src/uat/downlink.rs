//! The UAT ADS-B message data block (DO-282B section 2.2.4.5): header, state vector and mode
//! status, read into the aircraft report the 1090 MHz decoder emits too.

use sdrmm_decoder::aircraft::AdsbMessage;
use sdrmm_dsp::bits_be;

const CALLSIGN_ALPHABET: &[u8; 40] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ  ..";
const ANGLE_SCALE: f64 = 360.0 / 16_777_216.0;
const ALTITUDE_STEP_FT: i32 = 25;
const ALTITUDE_FLOOR_FT: i32 = -1_000;
const VERTICAL_RATE_STEP_FPM: i32 = 64;
const SUPERSONIC_FACTOR: i32 = 4;
const TRACK_STEPS: f64 = 512.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum AirGround {
    Subsonic,
    Supersonic,
    Ground,
    Reserved,
}

/// The aircraft report a corrected Basic or Long frame carries. `raw` is the frame in hex, as
/// dump978 prints it.
pub(crate) fn report(frame: &[u8]) -> AdsbMessage {
    let payload_type = frame[0] >> 3;
    let mut message = AdsbMessage {
        icao: format!("{:06X}", bits_be(frame, 8, 24)),
        raw: hex_lower(frame),
        ..AdsbMessage::default()
    };
    state_vector(frame, &mut message);
    if matches!(payload_type, 1 | 3) && frame.len() > 26 {
        mode_status(frame, &mut message);
    }
    message
}

fn state_vector(frame: &[u8], message: &mut AdsbMessage) {
    let lat = bits_be(frame, 32, 23);
    let lon = bits_be(frame, 55, 24);
    let nic = bits_be(frame, 92, 4);
    if nic != 0 || lat != 0 || lon != 0 {
        let lat = lat as f64 * ANGLE_SCALE;
        let lon = lon as f64 * ANGLE_SCALE;
        message.lat = Some(if lat > 90.0 { lat - 180.0 } else { lat });
        message.lon = Some(if lon > 180.0 { lon - 360.0 } else { lon });
    }
    let altitude = bits_be(frame, 80, 12) as i32;
    if altitude != 0 {
        message.altitude_ft = Some((altitude - 1) * ALTITUDE_STEP_FT + ALTITUDE_FLOOR_FT);
    }
    let air_ground = match bits_be(frame, 96, 2) {
        0 => AirGround::Subsonic,
        1 => AirGround::Supersonic,
        2 => AirGround::Ground,
        _ => AirGround::Reserved,
    };
    match air_ground {
        AirGround::Subsonic | AirGround::Supersonic => {
            message.on_ground = Some(false);
            airborne_velocity(frame, air_ground == AirGround::Supersonic, message);
        }
        AirGround::Ground => {
            message.on_ground = Some(true);
            ground_movement(frame, message);
        }
        AirGround::Reserved => {}
    }
}

fn signed_velocity(raw: u32, factor: i32) -> Option<i32> {
    let magnitude = (raw & 0x3FF) as i32;
    (magnitude != 0).then(|| {
        let speed = (magnitude - 1) * factor;
        if raw & 0x400 != 0 { -speed } else { speed }
    })
}

fn airborne_velocity(frame: &[u8], supersonic: bool, message: &mut AdsbMessage) {
    let factor = if supersonic { SUPERSONIC_FACTOR } else { 1 };
    let north = signed_velocity(bits_be(frame, 99, 11) as u32, factor);
    let east = signed_velocity(bits_be(frame, 110, 11) as u32, factor);
    if let (Some(north), Some(east)) = (north, east) {
        let (north, east) = (f64::from(north), f64::from(east));
        message.ground_speed_kt = Some(north.hypot(east));
        if north != 0.0 || east != 0.0 {
            message.track_deg = Some(east.atan2(north).to_degrees().rem_euclid(360.0));
        }
    }
    let vertical = bits_be(frame, 121, 11) as u32;
    let rate = (vertical & 0x1FF) as i32;
    if rate != 0 {
        let fpm = (rate - 1) * VERTICAL_RATE_STEP_FPM;
        message.vertical_rate_fpm = Some(if vertical & 0x200 != 0 { -fpm } else { fpm });
    }
}

fn ground_movement(frame: &[u8], message: &mut AdsbMessage) {
    let speed = bits_be(frame, 99, 11) as i32 & 0x3FF;
    if speed != 0 {
        message.ground_speed_kt = Some(f64::from(speed - 1));
    }
    let track = bits_be(frame, 110, 11) as u32;
    if track >> 9 == 1 {
        message.track_deg = Some(f64::from(track & 0x1FF) * 360.0 / TRACK_STEPS);
    }
}

fn mode_status(frame: &[u8], message: &mut AdsbMessage) {
    let mut letters = [b' '; 8];
    for (word, at) in [(0usize, 0usize), (1, 2), (2, 5)] {
        let value = usize::from(u16::from_be_bytes([
            frame[17 + 2 * word],
            frame[18 + 2 * word],
        ]));
        let chars = [value / 1600 % 40, value / 40 % 40, value % 40];
        let skip = usize::from(word == 0);
        for (slot, &index) in letters[at..].iter_mut().zip(&chars[skip..]) {
            *slot = CALLSIGN_ALPHABET[index];
        }
    }
    let text = String::from_utf8_lossy(&letters).trim_end().to_owned();
    if text.is_empty() {
        return;
    }
    if frame[26] & 0x02 != 0 {
        message.callsign = Some(text);
    } else {
        message.squawk = Some(text);
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex"))
            .collect()
    }

    fn near(value: Option<f64>, expected: f64, tolerance: f64) -> bool {
        value.is_some_and(|value| (value - expected).abs() < tolerance)
    }

    /// dump978 sample data, line 1: `uat2text` reads 37.4534 -122.0964, 1000 ft, N/S -99 kt,
    /// E/W 65 kt, track 146, 118 kt, -192 ft/min.
    #[test]
    fn a_basic_frame_reads_as_dump978_reads_it() {
        let message = report(&hex("00a66ef135445d525a0c0519119021204800"));
        assert_eq!(message.icao, "A66EF1");
        assert_eq!(message.df, None);
        assert!(near(message.lat, 37.4534, 1e-4), "{message:?}");
        assert!(near(message.lon, -122.0964, 1e-4), "{message:?}");
        assert_eq!(message.altitude_ft, Some(1_000));
        assert!(near(message.ground_speed_kt, 118.0, 1.0), "{message:?}");
        assert!(near(message.track_deg, 146.0, 1.0), "{message:?}");
        assert_eq!(message.vertical_rate_fpm, Some(-192));
        assert_eq!(message.on_ground, Some(false));
        assert_eq!(message.callsign, None);
    }

    /// Line 1003: AD7233 squawking 0325 at 3175 ft.
    #[test]
    fn a_long_frame_carries_its_squawk() {
        let message = report(&hex(
            "08ad7233358c435286720a89009a36202f06430d6ded2d0bcea4c080000ae0000000",
        ));
        assert_eq!(message.icao, "AD7233");
        assert_eq!(message.squawk.as_deref(), Some("0325"));
        assert_eq!(message.callsign, None);
        assert_eq!(message.altitude_ft, Some(3_175));
        assert!(near(message.lat, 37.6509, 1e-4), "{message:?}");
        assert!(near(message.lon, -121.9745, 1e-4), "{message:?}");
        assert!(near(message.track_deg, 289.0, 1.0), "{message:?}");
        assert_eq!(message.vertical_rate_fpm, Some(-64));
    }

    /// Line 547: N70FC.
    #[test]
    fn a_long_frame_carries_its_callsign() {
        let message = report(&hex(
            "0aa952b5358a57523ff005a813b802806f039f0264e6c404b0970200000620000000",
        ));
        assert_eq!(message.icao, "A952B5");
        assert_eq!(message.callsign.as_deref(), Some("N70FC"));
        assert_eq!(message.squawk, None);
    }

    #[test]
    fn a_ground_report_reads_speed_and_true_track() {
        let mut frame = [0u8; 18];
        frame[12] = 0x80;
        frame[13] = 0x41;
        frame[14] = 0x40;
        let message = report(&frame);
        assert_eq!(message.on_ground, Some(true));
        assert_eq!(message.ground_speed_kt, Some(15.0));
        assert_eq!(message.track_deg, Some(90.0));
    }
}

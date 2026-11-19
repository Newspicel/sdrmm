//! UAT bursts for tests and the signal generator: ADS-B frames from an aircraft's state and
//! FIS-B uplinks carrying DLAC text, keyed as binary CPFSK with modulation index 0.6.

use num_complex::Complex;
use sdrmm_modem::synth::{Render, Signal, fsk};

use super::fec::{Codes, LONG_DATA, UPLINK_DATA};
use super::receiver::{ADSB_SYNC, SYNC_BITS, UPLINK_SYNC};
use super::uplink::DLAC;
use super::{BIT_RATE_HZ, INPUT_RATE_HZ};

const DEVIATION_HZ: f64 = 312_500.0;
const ANGLE_SCALE: f64 = 16_777_216.0 / 360.0;
const GAP_S: f64 = 0.002;
const GENERIC_TEXT: u16 = 413;
const RECORD_SEPARATOR: u8 = 29;
const ETX: u8 = 0;
const SPACE: u8 = 32;

/// An airborne aircraft as a Long ADS-B frame would describe it.
#[derive(Clone, Copy, Debug)]
pub struct Aircraft<'a> {
    pub address: u32,
    pub callsign: &'a str,
    pub lat: f64,
    pub lon: f64,
    pub altitude_ft: i32,
    pub north_kt: i32,
    pub east_kt: i32,
    pub climb_fpm: i32,
}

/// A burst on the air: the frame's data, before the code's parity.
#[derive(Clone, Debug)]
pub enum Burst {
    Downlink(Vec<u8>),
    Uplink(Box<[u8; UPLINK_DATA]>),
}

fn put(frame: &mut [u8], offset: usize, width: usize, value: u32) {
    for bit in 0..width {
        let at = offset + bit;
        let set = value >> (width - 1 - bit) & 1 == 1;
        frame[at / 8] = frame[at / 8] & !(0x80 >> (at % 8)) | u8::from(set) << (7 - at % 8);
    }
}

fn signed(value: i32, magnitude_bits: usize) -> u32 {
    (value.unsigned_abs() + 1) | (u32::from(value < 0) << magnitude_bits)
}

/// A Long frame of payload type 1: state vector, mode status with a callsign, and no
/// secondary altitude.
#[must_use]
pub fn long_frame(aircraft: &Aircraft<'_>) -> Vec<u8> {
    let mut frame = vec![0u8; LONG_DATA];
    put(&mut frame, 0, 5, 1);
    put(&mut frame, 8, 24, aircraft.address);
    let lat = (aircraft.lat.rem_euclid(180.0) * ANGLE_SCALE).round() as u32;
    let lon = (aircraft.lon.rem_euclid(360.0) * ANGLE_SCALE).round() as u32;
    put(&mut frame, 32, 23, lat);
    put(&mut frame, 55, 24, lon);
    put(
        &mut frame,
        80,
        12,
        ((aircraft.altitude_ft + 1_000) / 25 + 1) as u32,
    );
    put(&mut frame, 92, 4, 8);
    put(&mut frame, 99, 11, signed(aircraft.north_kt, 10));
    put(&mut frame, 110, 11, signed(aircraft.east_kt, 10));
    put(
        &mut frame,
        121,
        11,
        signed(aircraft.climb_fpm / 64, 9) | 0x400,
    );
    put_callsign(&mut frame, aircraft.callsign);
    frame[26] |= 0x02;
    frame
}

fn put_callsign(frame: &mut [u8], callsign: &str) {
    let mut codes = [36u32; 8];
    for (slot, letter) in codes.iter_mut().zip(callsign.bytes()) {
        *slot = match letter {
            b'0'..=b'9' => u32::from(letter - b'0'),
            b'A'..=b'Z' => u32::from(letter - b'A') + 10,
            _ => 36,
        };
    }
    let emitter = 1;
    let words = [
        emitter * 1600 + codes[0] * 40 + codes[1],
        codes[2] * 1600 + codes[3] * 40 + codes[4],
        codes[5] * 1600 + codes[6] * 40 + codes[7],
    ];
    for (index, word) in words.into_iter().enumerate() {
        put(frame, 136 + 16 * index, 16, word);
    }
}

/// An uplink whose application data holds one FIS-B generic text APDU per report.
#[must_use]
pub fn text_uplink(reports: &[&str]) -> Box<[u8; UPLINK_DATA]> {
    let mut data = Box::new([0u8; UPLINK_DATA]);
    data[6] = 0x20;
    let mut at = 8;
    for report in reports {
        let mut apdu = vec![0u8; 4];
        put(&mut apdu, 3, 11, u32::from(GENERIC_TEXT));
        put(&mut apdu, 17, 5, 12);
        put(&mut apdu, 22, 6, 30);
        apdu.extend(dlac(report));
        put(&mut data[at..], 0, 9, apdu.len() as u32);
        data[at + 2..at + 2 + apdu.len()].copy_from_slice(&apdu);
        at += 2 + apdu.len();
    }
    data
}

/// `text` in DLAC with a record separator, padded with ETX to whole bytes.
#[must_use]
pub fn dlac(text: &str) -> Vec<u8> {
    let mut codes: Vec<u8> = text
        .chars()
        .map(|letter| {
            DLAC.iter()
                .position(|&code| char::from(code) == letter)
                .map_or(SPACE, |code| code as u8)
        })
        .collect();
    codes.push(RECORD_SEPARATOR);
    let mut out = Vec::with_capacity(codes.len() * 3 / 4 + 3);
    for group in codes.chunks(4) {
        let mut word = [ETX; 4];
        word[..group.len()].copy_from_slice(group);
        out.extend_from_slice(&[
            word[0] << 2 | word[1] >> 4,
            word[1] << 4 | word[2] >> 2,
            word[2] << 6 | word[3],
        ]);
    }
    out
}

fn bits(sync: u64, bytes: &[u8]) -> Vec<bool> {
    let sync = (0..SYNC_BITS).rev().map(|bit| sync >> bit & 1 == 1);
    let data = bytes
        .iter()
        .flat_map(|&byte| (0..8).rev().map(move |bit| byte >> bit & 1 == 1));
    sync.chain(data).collect()
}

/// Every burst keyed in turn at `rate`, each after a short gap.
#[must_use]
pub fn transmission(bursts: &[Burst], rate: f64) -> Vec<Complex<f32>> {
    let codes = Codes::new();
    let gap = vec![Complex::new(0.0, 0.0); (GAP_S * rate) as usize];
    let mut iq = gap.clone();
    for burst in bursts {
        let keyed = match burst {
            Burst::Downlink(data) => bits(ADSB_SYNC, &codes.encode_downlink(data)),
            Burst::Uplink(data) => bits(UPLINK_SYNC, &codes.encode_uplink(data)),
        };
        iq.extend(fsk(&keyed, BIT_RATE_HZ, DEVIATION_HZ, rate));
        iq.extend_from_slice(&gap);
    }
    iq
}

/// One aircraft and one ground station's METAR.
#[must_use]
pub fn scene(rate: f64) -> Vec<Complex<f32>> {
    transmission(
        &[
            Burst::Downlink(long_frame(&N70FC)),
            Burst::Uplink(text_uplink(&[METAR])),
        ],
        rate,
    )
}

pub const N70FC: Aircraft<'static> = Aircraft {
    address: 0xA9_52B5,
    callsign: "N70FC",
    lat: 37.4613,
    lon: -122.1146,
    altitude_ft: 2_500,
    north_kt: -40,
    east_kt: 90,
    climb_fpm: 512,
};

pub const METAR: &str = "METAR KPAO 241547Z 31008KT 10SM CLR 14/02 A3012=";

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "uat_traffic",
    label: "UAT traffic and FIS-B",
    rate_hz: INPUT_RATE_HZ,
    render: Render::Always(uat_traffic),
}];

fn uat_traffic() -> Vec<Complex<f32>> {
    scene(INPUT_RATE_HZ)
}

mod wifi;

use std::f64::consts::{PI, TAU};

use num_complex::Complex;
use sdrmm_wire::{RemoteIdMessage, RemoteIdPhy};

pub use self::wifi::{WifiRate, dsss, ofdm};
use crate::remote_id::{
    ble::{
        self, ACCESS_ADDRESS, CRC_BYTES,
        coded::{Encoder, ci_bits, encode_s8},
    },
    odid::{self, APP_CODE, SERVICE_UUID},
};

pub use crate::remote_id::{odid::encode, wifi::build};

const SYMBOL_RATE: f64 = 1_000_000.0;
const DEVIATION_HZ: f64 = 250_000.0;
const BT: f64 = 0.5;
const PULSE_SPAN: f64 = 2.0;
const ADV_NONCONN_IND: u8 = 0x02;
const ADV_EXT_IND: u8 = 0x07;
const TX_ADD_RANDOM: u8 = 0x40;
const CODED_PREAMBLE: [bool; 8] = [false, false, true, true, true, true, false, false];
const CODED_PREAMBLE_REPEATS: usize = 10;
const TERM_BITS: usize = 3;

fn service_data(counter: u8, payload: &[u8]) -> Vec<u8> {
    let uuid = SERVICE_UUID.to_le_bytes();
    let mut out = vec![
        (payload.len() + 5) as u8,
        0x16,
        uuid[0],
        uuid[1],
        APP_CODE,
        counter,
    ];
    out.extend_from_slice(payload);
    out
}

#[must_use]
pub fn legacy_pdu(address: [u8; 6], counter: u8, message: &RemoteIdMessage) -> Vec<u8> {
    let data = service_data(counter, &odid::encode::message(message));
    let mut pdu = vec![ADV_NONCONN_IND | TX_ADD_RANDOM, (6 + data.len()) as u8];
    pdu.extend(address.iter().rev());
    pdu.extend_from_slice(&data);
    pdu
}

#[must_use]
pub fn extended_pdu(address: [u8; 6], counter: u8, messages: &[RemoteIdMessage]) -> Vec<u8> {
    let data = service_data(counter, &odid::encode::pack(messages));
    let header = [0x09u8];
    let mut payload = vec![(header.len() + 6 + 2) as u8];
    payload.extend_from_slice(&header);
    payload.extend(address.iter().rev());
    payload.extend_from_slice(&[0x75, 0x0E]);
    payload.extend_from_slice(&data);
    let mut pdu = vec![ADV_EXT_IND | TX_ADD_RANDOM, payload.len() as u8];
    pdu.extend_from_slice(&payload);
    pdu
}

fn with_crc(pdu: &[u8], channel_index: u8) -> Vec<u8> {
    let crc = ble::crc24(pdu);
    let mut out = pdu.to_vec();
    out.extend_from_slice(&crc.to_le_bytes()[..CRC_BYTES]);
    ble::whiten(channel_index, &mut out);
    out
}

fn lsb_bits(bytes: &[u8]) -> impl Iterator<Item = bool> + '_ {
    bytes
        .iter()
        .flat_map(|&byte| (0..8).map(move |bit| byte >> bit & 1 == 1))
}

#[must_use]
pub fn le_1m_symbols(pdu: &[u8], channel_index: u8) -> Vec<bool> {
    let mut symbols: Vec<bool> = (0..8).map(|bit| bit % 2 == 1).collect();
    symbols.extend((0..32).map(|bit| ACCESS_ADDRESS >> bit & 1 == 1));
    symbols.extend(lsb_bits(&with_crc(pdu, channel_index)));
    symbols
}

#[must_use]
pub fn le_coded_symbols(pdu: &[u8], channel_index: u8, phy: RemoteIdPhy) -> Vec<bool> {
    let mut symbols: Vec<bool> = CODED_PREAMBLE
        .iter()
        .copied()
        .cycle()
        .take(CODED_PREAMBLE.len() * CODED_PREAMBLE_REPEATS)
        .collect();
    let mut encoder = Encoder::default();
    let first_block = (0..32)
        .map(|bit| ACCESS_ADDRESS >> bit & 1 == 1)
        .chain(ci_bits(phy))
        .chain(std::iter::repeat_n(false, TERM_BITS));
    symbols.extend(encode_s8(&mut encoder, first_block));
    let mut encoder = Encoder::default();
    let payload = with_crc(pdu, channel_index);
    let second_block = lsb_bits(&payload).chain(std::iter::repeat_n(false, TERM_BITS));
    if phy == RemoteIdPhy::LeCodedS8 {
        symbols.extend(encode_s8(&mut encoder, second_block));
    } else {
        symbols.extend(second_block.flat_map(|bit| encoder.push(bit)));
    }
    symbols
}

fn erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let value = 1.0 - poly * (-x * x).exp();
    if x < 0.0 { -value } else { value }
}

fn gaussian_pulse(t: f64) -> f64 {
    let k = PI * BT * (2.0 / 2f64.ln()).sqrt();
    0.5 * (erf(k * (t + 0.5)) - erf(k * (t - 0.5)))
}

#[must_use]
pub fn gfsk(symbols: &[bool], rate: f64) -> Vec<Complex<f32>> {
    let samples_per_symbol = rate / SYMBOL_RATE;
    let len = ((symbols.len() as f64 + 2.0) * samples_per_symbol) as usize;
    let mut phase = 0.0f64;
    (0..len)
        .map(|n| {
            let t = n as f64 / samples_per_symbol - 0.5;
            let centre = t.round() as isize;
            let reach = PULSE_SPAN as isize;
            let frequency: f64 = (centre - reach..=centre + reach)
                .filter_map(|k| {
                    let symbol = *symbols.get(usize::try_from(k).ok()?)?;
                    let sign = if symbol { 1.0 } else { -1.0 };
                    Some(sign * gaussian_pulse(t - k as f64))
                })
                .sum();
            phase = (phase + TAU * DEVIATION_HZ * frequency / rate).rem_euclid(TAU);
            Complex::from_polar(1.0, phase as f32)
        })
        .collect()
}

#[must_use]
pub fn bluetooth(pdu: &[u8], rf_channel: u8, phy: RemoteIdPhy, rate: f64) -> Vec<Complex<f32>> {
    let channel_index = ble::channel_index(rf_channel);
    let symbols = match phy {
        RemoteIdPhy::LeCodedS8 | RemoteIdPhy::LeCodedS2 => {
            le_coded_symbols(pdu, channel_index, phy)
        }
        _ => le_1m_symbols(pdu, channel_index),
    };
    gfsk(&symbols, rate)
}

const SCENE_ADDRESS: [u8; 6] = [0xD4, 0x5A, 0x21, 0x0C, 0x7E, 0x19];
const SCENE_GAP_S: f64 = 0.02;

#[must_use]
pub fn demo_drone() -> Vec<RemoteIdMessage> {
    use sdrmm_wire::{
        EuCategory, EuClassification, HeightReference, OperatorLocationType, RemoteIdLocation,
        RemoteIdSystem, UaStatus, UaType, UasIdType,
    };
    vec![
        RemoteIdMessage::BasicId {
            id_type: UasIdType::SerialNumber,
            ua_type: UaType::Rotorcraft,
            uas_id: "1581F5FJD239C00DW22E".to_owned(),
        },
        RemoteIdMessage::Location(RemoteIdLocation {
            status: UaStatus::Airborne,
            lat: Some(52.516_3),
            lon: Some(13.377_7),
            track_deg: Some(74.0),
            speed_mps: Some(6.5),
            vertical_speed_mps: Some(0.5),
            pressure_altitude_m: Some(118.5),
            geodetic_altitude_m: Some(156.0),
            height_m: Some(84.0),
            height_reference: HeightReference::Takeoff,
            horizontal_accuracy_m: Some(3.0),
            vertical_accuracy_m: Some(10.0),
            pressure_accuracy_m: None,
            speed_accuracy_mps: Some(1.0),
            seconds_after_hour: Some(1_204.5),
            timestamp_accuracy_s: Some(0.1),
        }),
        RemoteIdMessage::SelfId {
            description_type: 0,
            text: "Roof survey".to_owned(),
        },
        RemoteIdMessage::System(RemoteIdSystem {
            operator_location_type: OperatorLocationType::LiveGnss,
            operator_lat: Some(52.515_6),
            operator_lon: Some(13.376_1),
            operator_altitude_m: Some(71.0),
            area_count: 1,
            area_radius_m: 0,
            area_ceiling_m: None,
            area_floor_m: None,
            classification: Some(EuClassification {
                category: EuCategory::Open,
                class: Some(1),
            }),
            timestamp: Some(1_791_460_800),
        }),
        RemoteIdMessage::OperatorId {
            id_type: 0,
            operator_id: "DEUx2b9q1k4m7n3p".to_owned(),
        },
    ]
}

fn gap(rate: f64) -> Vec<Complex<f32>> {
    vec![Complex::new(0.0, 0.0); (SCENE_GAP_S * rate) as usize]
}

#[must_use]
pub fn bluetooth_scene(rate: f64) -> Vec<Complex<f32>> {
    let mut iq = gap(rate);
    for (counter, message) in demo_drone().iter().enumerate() {
        let pdu = legacy_pdu(SCENE_ADDRESS, counter as u8, message);
        iq.extend(bluetooth(&pdu, 0, RemoteIdPhy::Le1m, rate));
        iq.extend(gap(rate));
    }
    let pdu = extended_pdu(SCENE_ADDRESS, 5, &demo_drone());
    iq.extend(bluetooth(&pdu, 0, RemoteIdPhy::LeCodedS8, rate));
    iq.extend(gap(rate));
    iq
}

#[must_use]
pub fn wifi_scene() -> Vec<Complex<f32>> {
    let rate = sdrmm_wire::RemoteIdLink::Wifi.input_rate_hz();
    let pack = encode::pack(&demo_drone());
    let mut iq = gap(rate);
    iq.extend(ofdm(
        &build::beacon(SCENE_ADDRESS, "RID-DW22E", 1, &pack),
        6,
    ));
    iq.extend(gap(rate));
    iq.extend(dsss(
        &build::nan(SCENE_ADDRESS, 2, &pack),
        RemoteIdPhy::Dsss1m,
        false,
    ));
    iq.extend(gap(rate));
    iq
}

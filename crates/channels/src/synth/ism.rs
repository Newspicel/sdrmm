use std::f64::consts::{PI, TAU};

use num_complex::Complex;

use super::remote_id::{BlePhy, bluetooth, ofdm};

const ZIGBEE_CHIP_RATE: f64 = 2e6;
const SCENE_RATE: f64 = 20e6;
const SCENE_S: f64 = 0.1;
const WIFI_PERIOD_S: f64 = 0.004;
const BLE_PERIOD_S: f64 = 0.02;
const BLE_OFFSET_HZ: f64 = 3e6;
const ZIGBEE_PERIOD_S: f64 = 0.05;
const ZIGBEE_OFFSET_HZ: f64 = -2e6;
const ZIGBEE_S: f64 = 1e-3;
const OVEN_S: f64 = 9e-3;
const MAINS_S: f64 = 0.02;
const OVEN_OFFSET_HZ: f64 = 6e6;

#[must_use]
pub fn wifi_frame(bytes: usize) -> Vec<Complex<f32>> {
    let mpdu = sdrmm_modem::wifi::append_fcs((0..bytes).map(|k| (k * 37 + 11) as u8).collect());
    ofdm(&mpdu, 24)
}

#[must_use]
pub fn ble_advert(rate: f64) -> Vec<Complex<f32>> {
    let mut pdu = vec![0x42, 30];
    pdu.extend([1, 2, 3, 4, 5, 0xC6]);
    pdu.extend((0..24).map(|k| k as u8));
    bluetooth(&pdu, 19, BlePhy::Le1m, rate)
}

#[must_use]
pub fn zigbee_frame(rate: f64, seconds: f64) -> Vec<Complex<f32>> {
    let chip = rate / ZIGBEE_CHIP_RATE;
    let len = (seconds * rate) as usize;
    let mut state = 0x1234_5678u32;
    let mut chips: Vec<f32> = Vec::new();
    while (chips.len() as f64) * chip < len as f64 + 4.0 * chip {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        chips.push(if state & 1 == 0 { 1.0 } else { -1.0 });
    }
    let pulse = |t: f64| {
        if (0.0..2.0).contains(&t) {
            (PI * t / 2.0).sin() as f32
        } else {
            0.0
        }
    };
    (0..len)
        .map(|n| {
            let t = n as f64 / chip;
            let pair = (t / 2.0).floor() as usize;
            let i = chips[2 * pair] * pulse(t - 2.0 * pair as f64);
            let late = t - 1.0;
            let q = if late >= 0.0 {
                let pair = (late / 2.0).floor() as usize;
                chips[2 * pair + 1] * pulse(late - 2.0 * pair as f64)
            } else {
                0.0
            };
            Complex::new(i, q)
        })
        .collect()
}

#[must_use]
pub fn oven_burst(rate: f64, seconds: f64) -> Vec<Complex<f32>> {
    let len = (seconds * rate) as usize;
    let tones: Vec<f64> = (0..20).map(|k| -3e6 + f64::from(k) * 300e3).collect();
    (0..len)
        .map(|n| {
            tones
                .iter()
                .enumerate()
                .map(|(k, &hz)| {
                    Complex::from_polar(0.05, (TAU * hz * n as f64 / rate + k as f64) as f32)
                })
                .sum()
        })
        .collect()
}

fn place(iq: &mut [Complex<f32>], start_s: f64, offset_hz: f64, gain: f32, burst: &[Complex<f32>]) {
    let start = (start_s * SCENE_RATE) as usize;
    let step = TAU * offset_hz / SCENE_RATE;
    for (index, &sample) in burst.iter().enumerate() {
        if let Some(slot) = iq.get_mut(start + index) {
            *slot += sample * Complex::from_polar(gain, (step * index as f64) as f32);
        }
    }
}

fn every(
    iq: &mut [Complex<f32>],
    first_s: f64,
    period_s: f64,
    offset_hz: f64,
    gain: f32,
    burst: &[Complex<f32>],
) {
    let mut start = first_s;
    while start < SCENE_S {
        place(iq, start, offset_hz, gain, burst);
        start += period_s;
    }
}

#[must_use]
pub fn scene() -> Vec<Complex<f32>> {
    let mut iq = vec![Complex::new(0.0, 0.0); (SCENE_S * SCENE_RATE) as usize];
    every(&mut iq, 0.001, WIFI_PERIOD_S, 0.0, 0.1, &wifi_frame(300));
    every(
        &mut iq,
        0.0025,
        BLE_PERIOD_S,
        BLE_OFFSET_HZ,
        0.1,
        &ble_advert(SCENE_RATE),
    );
    every(
        &mut iq,
        0.012,
        ZIGBEE_PERIOD_S,
        ZIGBEE_OFFSET_HZ,
        0.1,
        &zigbee_frame(SCENE_RATE, ZIGBEE_S),
    );
    every(
        &mut iq,
        0.03,
        MAINS_S * 4.0,
        OVEN_OFFSET_HZ,
        1.0,
        &oven_burst(SCENE_RATE, OVEN_S),
    );
    iq
}

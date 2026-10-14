use num_complex::Complex;
use sdrmm_wire::{WmbusFormat, WmbusMode};

use crate::{
    synth,
    wmbus::{coding::encode_three_of_six, link},
};

const FAST_CHIPS_PER_S: f64 = 100_000.0;
const SLOW_CHIPS_PER_S: f64 = 32_768.0;
const DEVIATION_HZ: f64 = 50_000.0;
const T_PREAMBLE_PAIRS: usize = 19;
const C_PREAMBLE_PAIRS: usize = 16;
const S_PREAMBLE_PAIRS: usize = 40;
const T_SYNC: &[u8] = b"0000111101";
const S_SYNC: &[u8] = b"000111011010010110";
const C_SYNC: u16 = 0x543D;
const C_FORMAT_A: u16 = 0x54CD;
const POSTAMBLE: &[u8] = b"0101";
const GUARD_S: f64 = 0.001;

pub const WATER_METER: [u8; 25] = [
    0x18, 0x44, 0xAE, 0x4C, 0x44, 0x55, 0x22, 0x33, 0x68, 0x07, 0x7A, 0x55, 0x00, 0x00, 0x00, 0x04,
    0x13, 0x89, 0xE2, 0x01, 0x00, 0x02, 0x3B, 0x00, 0x00,
];

fn push_text(chips: &mut Vec<bool>, text: &[u8]) {
    chips.extend(text.iter().map(|&chip| chip == b'1'));
}

fn push_bits(chips: &mut Vec<bool>, word: u32, bits: u32) {
    chips.extend((0..bits).rev().map(|k| word >> k & 1 == 1));
}

fn preamble(chips: &mut Vec<bool>, pairs: usize) {
    for _ in 0..pairs {
        push_text(chips, b"01");
    }
}

#[must_use]
pub fn chips(mode: WmbusMode, format: WmbusFormat, payload: &[u8]) -> Vec<bool> {
    let raw = link::frame(format, payload);
    let mut chips = Vec::new();
    match mode {
        WmbusMode::T => {
            preamble(&mut chips, T_PREAMBLE_PAIRS);
            push_text(&mut chips, T_SYNC);
            for &byte in &raw {
                for code in encode_three_of_six(byte) {
                    push_bits(&mut chips, code.into(), 6);
                }
            }
        }
        WmbusMode::C => {
            preamble(&mut chips, C_PREAMBLE_PAIRS);
            push_bits(&mut chips, C_SYNC.into(), 16);
            let marker = match format {
                WmbusFormat::A => C_FORMAT_A,
                WmbusFormat::B => C_SYNC,
            };
            push_bits(&mut chips, marker.into(), 16);
            for &byte in &raw {
                push_bits(&mut chips, byte.into(), 8);
            }
        }
        WmbusMode::S => {
            preamble(&mut chips, S_PREAMBLE_PAIRS);
            push_text(&mut chips, S_SYNC);
            for &byte in &raw {
                for k in (0..8).rev() {
                    push_text(&mut chips, if byte >> k & 1 == 1 { b"01" } else { b"10" });
                }
            }
        }
    }
    push_text(&mut chips, POSTAMBLE);
    chips
}

#[must_use]
pub fn telegram(
    mode: WmbusMode,
    format: WmbusFormat,
    payload: &[u8],
    rate: f64,
) -> Vec<Complex<f32>> {
    let chip_rate = match mode {
        WmbusMode::S => SLOW_CHIPS_PER_S,
        WmbusMode::T | WmbusMode::C => FAST_CHIPS_PER_S,
    };
    let guard = (GUARD_S * rate) as usize;
    let mut iq = synth::silence(guard);
    iq.extend(synth::fsk(
        &chips(mode, format, payload),
        chip_rate,
        DEVIATION_HZ,
        rate,
    ));
    iq.extend(synth::silence(guard));
    iq
}

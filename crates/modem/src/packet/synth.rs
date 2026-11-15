use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::crc16_x25;

use super::{AFSK_BAUD, AFSK_MARK_HZ, AFSK_SPACE_HZ, DEVIATION_HZ};
use crate::synth::fm_modulate;

const FLAG: u8 = 0x7E;
const PREAMBLE_FLAGS: usize = 24;
const TRAILING_FLAGS: usize = 2;
const STUFF_AFTER_ONES: usize = 5;

/// `frame` as one 1200 baud AFSK transmission on FM: flags, the frame with its FCS, bit-stuffed
/// and NRZI-coded, then closing flags.
#[must_use]
pub fn afsk_burst(frame: &[u8], rate: f64) -> Vec<Complex<f32>> {
    let mut with_fcs = frame.to_vec();
    with_fcs.extend(crc16_x25(frame).to_le_bytes());
    let mut bits = flags(PREAMBLE_FLAGS);
    bits.extend(stuffed(&with_fcs));
    bits.extend(flags(TRAILING_FLAGS));
    fm_modulate(&afsk(&nrzi(&bits), rate), DEVIATION_HZ, rate)
}

fn flags(count: usize) -> Vec<bool> {
    (0..count * 8).map(|k| FLAG >> (k % 8) & 1 == 1).collect()
}

fn stuffed(bytes: &[u8]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(bytes.len() * 9);
    let mut ones = 0;
    for bit in bytes
        .iter()
        .flat_map(|&b| (0..8).map(move |i| b >> i & 1 == 1))
    {
        bits.push(bit);
        ones = if bit { ones + 1 } else { 0 };
        if ones == STUFF_AFTER_ONES {
            bits.push(false);
            ones = 0;
        }
    }
    bits
}

fn nrzi(bits: &[bool]) -> Vec<bool> {
    let mut level = false;
    bits.iter()
        .map(|&bit| {
            level ^= !bit;
            level
        })
        .collect()
}

fn afsk(levels: &[bool], rate: f64) -> Vec<f32> {
    let per_bit = rate / AFSK_BAUD;
    let mut tone = 0.0f64;
    (0..(levels.len() as f64 * per_bit) as usize)
        .map(|k| {
            let mark = levels[((k as f64 / per_bit) as usize).min(levels.len() - 1)];
            tone += TAU * if mark { AFSK_MARK_HZ } else { AFSK_SPACE_HZ } / rate;
            tone.sin() as f32
        })
        .collect()
}

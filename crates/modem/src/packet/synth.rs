use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::{HdlcFramer, crc16_x25};

use super::{AFSK_BAUD, AFSK_MARK_HZ, AFSK_SPACE_HZ, DEVIATION_HZ, Line, PacketMode};
use crate::synth::fm_modulate;

const PREAMBLE_FLAGS: usize = 24;
const TRAILING_FLAGS: usize = 2;

/// `frame` as one 1200 baud AFSK transmission on FM: flags, the frame with its FCS, bit-stuffed
/// and NRZI-coded, then closing flags.
#[must_use]
pub fn afsk_burst(frame: &[u8], rate: f64) -> Vec<Complex<f32>> {
    let mut framer = HdlcFramer::new();
    let mut bits = Vec::new();
    framer.push_flags(&mut bits, PREAMBLE_FLAGS);
    framer.push_bytes(&mut bits, frame);
    framer.push_bytes(&mut bits, &crc16_x25(frame).to_le_bytes());
    framer.push_flags(&mut bits, TRAILING_FLAGS);
    fm_modulate(&afsk(&nrzi(&bits), rate), DEVIATION_HZ, rate)
}

fn nrzi(bits: &[bool]) -> Vec<bool> {
    let mut line = Line::new(PacketMode::Afsk1200);
    bits.iter().map(|&bit| line.push(bit)).collect()
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

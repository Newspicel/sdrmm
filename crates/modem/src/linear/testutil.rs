use std::f64::consts::TAU;

use num_complex::Complex;

use super::rotation::phasor;
use crate::{
    constellation::Constellation,
    pulse::{self, Norm},
};

pub const SPS: usize = 8;

const ROLL_OFF: f64 = 0.35;

const SPAN_SYMBOLS: usize = 8;

pub fn rrc() -> Vec<f32> {
    rrc_at(SPS)
}

pub fn rrc_at(sps: usize) -> Vec<f32> {
    pulse::root_raised_cosine(sps as f64, ROLL_OFF, SPAN_SYMBOLS, Norm::Energy)
}

/// Turns sample `k` by `phase0 + 2 pi cycles k`.
pub fn rotate(wave: &mut [Complex<f32>], cycles: f64, phase0: f64) {
    for (k, s) in wave.iter_mut().enumerate() {
        *s *= phasor(phase0 + TAU * cycles * k as f64);
    }
}

/// Symbols that slice to the wrong label, `got` aligned to the end of `sent`.
pub fn mis_sliced(table: &Constellation, got: &[Complex<f32>], sent: &[u32]) -> usize {
    let start = sent.len() - got.len();
    got.iter()
        .zip(&sent[start..])
        .filter(|&(&y, &want)| table.hard_slice(y) != want)
        .count()
}

/// RMS distance from each symbol to its nearest table point.
pub fn evm(table: &Constellation, symbols: &[Complex<f32>]) -> f64 {
    (symbols
        .iter()
        .map(|&y| f64::from((y - table.nearest(y)).norm_sqr()))
        .sum::<f64>()
        / symbols.len() as f64)
        .sqrt()
}

use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::math::special::parabolic_peak;

use crate::linear::{acquire::FrequencyAcquisition, rotation::phasor};

/// The sampling phase, in samples, whose derotated symbols carry the strongest `order`-th power
/// line.
pub fn rotated_power_offset(
    filtered: &[Complex<f32>],
    sps: usize,
    rotation_rad: f64,
    order: u32,
    acquisition: &mut FrequencyAcquisition,
    symbols: &mut Vec<Complex<f32>>,
) -> f64 {
    let mut metric = |offset: usize| -> f64 {
        symbols.clear();
        symbols.extend(
            (offset..filtered.len())
                .step_by(sps)
                .enumerate()
                .map(|(k, n)| filtered[n] * phasor((-rotation_rad * k as f64).rem_euclid(TAU))),
        );
        acquisition.peak_power(symbols, order).sqrt()
    };
    let scores: Vec<f64> = (0..sps).map(&mut metric).collect();
    let Some((best, _)) = scores.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)) else {
        return 0.0;
    };
    let shift = parabolic_peak(
        scores[(best + sps - 1) % sps],
        scores[best],
        scores[(best + 1) % sps],
    );
    (best as f64 + shift).rem_euclid(sps as f64)
}

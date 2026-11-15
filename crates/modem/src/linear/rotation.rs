use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::math::complex::narrow;

#[must_use]
pub(super) fn phasor(theta: f64) -> Complex<f32> {
    narrow(Complex::from_polar(1.0, theta))
}

/// Phase a per-symbol rotation schedule has reached at `symbol_time`, folded into one turn.
#[must_use]
pub(super) fn schedule_phase(symbol_time: f64, rotation_rad: f64) -> f64 {
    (symbol_time * rotation_rad) % TAU
}

#[must_use]
pub(super) fn derotate(y: Complex<f32>, symbol_time: f64, rotation_rad: f64) -> Complex<f32> {
    if rotation_rad == 0.0 {
        return y;
    }
    y * phasor(-schedule_phase(symbol_time, rotation_rad))
}

mod dechirp;
mod demod;
mod modulator;
mod params;
#[cfg(test)]
mod sync_tests;
#[cfg(test)]
mod tests;

use std::f64::consts::TAU;

use num_complex::Complex;

pub use self::{
    dechirp::{Dechirper, Peak, Slope, base_upchirp, fraction_of},
    demod::CssDemod,
    modulator::CssMod,
    params::CssParams,
};

pub const TRACK_BW: f64 = 0.02;

pub const TIMING_BW: f64 = 0.05;

pub const MAX_CLOCK_PPM: f64 = 1_000.0;

pub const MIN_SPREADING_FACTOR: u32 = 5;

pub const MAX_SPREADING_FACTOR: u32 = 12;

/// Phase, in turns, of an up-chirp over `n` chips at chip time `t`.
fn chirp_turns(t: f64, n: f64) -> f64 {
    t * t / (2.0 * n) - t / 2.0
}

/// `value` folded into `-period / 2..=period / 2`.
#[must_use]
pub fn wrap(value: f64, period: f64) -> f64 {
    value - (value / period).round() * period
}

/// The unit phasor `turns` turns round, reduced to one turn first to keep `f64` precision.
fn phasor(turns: f64) -> Complex<f32> {
    let (sin, cos) = (TAU * (turns - turns.floor())).sin_cos();
    Complex::new(cos as f32, sin as f32)
}

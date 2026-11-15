mod biquad;
mod dc_blocker;
mod deemphasis;
mod highpass;
mod iq_dc_blocker;
mod one_pole;
mod svf;

use std::f64::consts::TAU;

pub use biquad::Biquad;
pub use dc_blocker::DcBlocker;
pub use deemphasis::Deemphasis;
pub use highpass::Highpass;
pub use iq_dc_blocker::IqDcBlocker;
pub use one_pole::ComplexOnePole;
pub use svf::{Svf, SvfShape};

/// Samples the DC blockers run through their closed-form recursion at once.
const STRIDE: usize = 4;

#[must_use]
pub fn one_pole_coeff(rate: f64, tau_s: f64) -> f32 {
    (1.0 - (-1.0 / (rate * tau_s)).exp()) as f32
}

/// The one-pole coefficient whose corner sits at `corner_hz`.
fn corner_coeff(rate: f64, corner_hz: f64) -> f32 {
    one_pole_coeff(rate, 1.0 / (TAU * corner_hz))
}

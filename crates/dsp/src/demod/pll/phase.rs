use std::f64::consts::{PI, TAU};

use num_complex::Complex;

use crate::{math::fastmath::fast_arg, math::special::wrap_rad, tuning::nco::unit_phasor};

const INV_TAU: f64 = 1.0 / TAU;

/// The unit phasor at `phase` radians, from the oscillator's table rather than a library sine:
/// a loop turns it once per sample, and the table is accurate to a few parts in ten million.
#[inline]
pub(super) fn phasor(phase: f64) -> Complex<f32> {
    unit_phasor(phase * INV_TAU)
}

#[inline]
pub(super) fn phase_error(derotated: Complex<f32>) -> Option<f64> {
    (derotated.is_finite() && derotated.norm_sqr() > 0.0).then(|| f64::from(fast_arg(derotated)))
}

#[inline]
pub(super) fn step_phase(phase: f64, inc: f64) -> f64 {
    let next = phase + inc;
    if next >= PI {
        next - TAU
    } else if next < -PI {
        next + TAU
    } else {
        next
    }
}

/// A loop's increment is almost always in range already, so the library remainder only runs
/// when it is not.
#[inline]
pub(super) fn wrap_pi(x: f64) -> f64 {
    if (-PI..PI).contains(&x) {
        x
    } else {
        wrap_rad(x)
    }
}

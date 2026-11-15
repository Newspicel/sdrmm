use std::f64::consts::PI;

use super::{Geometry, Winding, wavelength_m, wavenumber};
use crate::testutil::uca5;

/// Radius of the five-element ring whose neighbours sit half a wavelength apart at 433.92 MHz.
pub(crate) const HALF_WAVE_UCA5_RADIUS_M: f64 = 0.2939;

pub(crate) fn half_wave_uca5() -> Geometry {
    uca5(HALF_WAVE_UCA5_RADIUS_M)
}

/// Ring of `count` elements whose radius is `beta` radians of phase at `freq_hz`.
pub(crate) fn uca_for_beta(count: usize, beta: f64, freq_hz: f64) -> Geometry {
    clockwise_ring(beta / wavenumber(freq_hz), count)
}

/// Ring of `count` elements whose neighbours sit `chord_wavelengths` apart at `freq_hz`.
pub(crate) fn uca_for_chord(count: usize, chord_wavelengths: f64, freq_hz: f64) -> Geometry {
    let chord = chord_wavelengths * wavelength_m(freq_hz);
    clockwise_ring(chord / (2.0 * (PI / count as f64).sin()), count)
}

fn clockwise_ring(radius_m: f64, count: usize) -> Geometry {
    Geometry::uca(radius_m, count, 0.0, Winding::Clockwise).unwrap()
}

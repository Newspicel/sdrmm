use std::f64::consts::TAU;

use num_complex::Complex;

use crate::linear::rotation::phasor;

/// A carrier frequency and its linear drift, both referred to the middle of the block.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Chirp {
    pub centre_cycles_per_symbol: f64,
    pub drift_cycles_per_symbol2: f64,
}

impl Chirp {
    pub fn remove(&self, symbols: &mut [Complex<f32>]) {
        self.remove_spaced(symbols, 1);
    }

    pub fn remove_spaced(&self, samples: &mut [Complex<f32>], per_symbol: usize) {
        let per_symbol = per_symbol.max(1);
        let centre = (samples.len().div_ceil(per_symbol) as f64 - 1.0) / 2.0;
        for (k, s) in samples.iter_mut().enumerate() {
            let t = k as f64 / per_symbol as f64 - centre;
            let cycles =
                self.centre_cycles_per_symbol * t + 0.5 * self.drift_cycles_per_symbol2 * t * t;
            *s *= phasor(-TAU * cycles.rem_euclid(1.0));
        }
    }
}

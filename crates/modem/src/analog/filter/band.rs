use num_complex::Complex;
use sdrmm_dsp::{Decimator, FirC, design_lowpass};

use super::odd_taps;

/// The band edges stay this far inside Nyquist, in cycles per sample.
const EDGE: f64 = 0.4999;
/// The narrowest band, so its edges never cross.
const GUARD: f64 = 1e-6;
/// A centre closer to DC than this builds a real filter.
const CENTRED: f64 = 1e-12;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandFilter {
    pub low: f64,
    pub high: f64,
    pub taps: usize,
}

impl BandFilter {
    #[must_use]
    pub fn symmetric(half_width: f64, taps: usize) -> Self {
        Self {
            low: -half_width,
            high: half_width,
            taps,
        }
    }

    #[must_use]
    pub fn build(&self) -> Band {
        let low = self.low.clamp(-EDGE, EDGE - GUARD);
        let high = self.high.clamp((low + GUARD).min(EDGE), EDGE);
        let taps = odd_taps(self.taps);
        let half_width = 0.5 * (high - low);
        let centre = 0.5 * (high + low);
        let prototype = design_lowpass(taps, half_width.min(EDGE));
        if centre.abs() < CENTRED {
            Band::Real(Decimator::new(&prototype, 1))
        } else {
            Band::Complex(FirC::from_lowpass(&prototype, centre))
        }
    }

    #[must_use]
    pub fn delay(&self) -> usize {
        odd_taps(self.taps) / 2
    }
}

pub enum Band {
    Real(Decimator),
    Complex(FirC),
}

impl Band {
    pub fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        match self {
            Self::Real(f) => f.process(iq, out),
            Self::Complex(f) => f.process(iq, out),
        }
    }
}

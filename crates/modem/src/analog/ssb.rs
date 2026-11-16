mod demod;
mod modulator;
mod weaver;

use sdrmm_dsp::{RealDecimator, design_bandpass, design_lowpass};

pub use self::{
    demod::{SsbDemod, SsbDetector},
    modulator::SsbMod,
};
use super::filter::BandFilter;

/// Taps of the band and audio filters: sharper than the other modes, for the opposite sideband.
const DEFAULT_TAPS: usize = 257;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sideband {
    Upper,
    Lower,
}

impl Sideband {
    #[must_use]
    pub fn sign(self) -> f64 {
        match self {
            Self::Upper => 1.0,
            Self::Lower => -1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SsbMethod {
    Hilbert,
    Weaver,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SsbParams {
    pub sideband: Sideband,
    pub method: SsbMethod,
    pub low_cut: f64,
    pub bandwidth: f64,
    pub band_taps: usize,
    pub audio_taps: usize,
}

impl SsbParams {
    #[must_use]
    pub fn new(sideband: Sideband, method: SsbMethod, bandwidth: f64) -> Self {
        Self {
            sideband,
            method,
            low_cut: 0.0,
            bandwidth,
            band_taps: DEFAULT_TAPS,
            audio_taps: DEFAULT_TAPS,
        }
    }

    #[must_use]
    pub fn band(&self) -> BandFilter {
        let (low, high) = (self.low_cut, self.bandwidth);
        match self.sideband {
            Sideband::Upper => BandFilter {
                low,
                high,
                taps: self.band_taps,
            },
            Sideband::Lower => BandFilter {
                low: -high,
                high: -low,
                taps: self.band_taps,
            },
        }
    }

    pub(super) fn weaver(&self) -> (f64, f64) {
        (
            f64::midpoint(self.bandwidth, self.low_cut),
            0.5 * (self.bandwidth - self.low_cut),
        )
    }

    pub(super) fn message_filter(&self) -> RealDecimator {
        let taps = if self.low_cut > 0.0 {
            design_bandpass(self.audio_taps, self.low_cut, self.bandwidth)
        } else {
            design_lowpass(self.audio_taps, self.bandwidth)
        };
        RealDecimator::new(&taps, 1)
    }
}

#[cfg(test)]
mod tests;

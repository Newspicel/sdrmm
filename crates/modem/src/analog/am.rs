mod demod;
mod modulator;

pub use self::{
    demod::{AmDemod, AmDetector, AmRx},
    modulator::AmMod,
};
use super::{filter::BandFilter, stage::DEFAULT_TAPS};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AmMode {
    FullCarrier { depth: f64 },
    Suppressed,
}

impl AmMode {
    #[must_use]
    pub fn depth(self) -> f64 {
        match self {
            Self::FullCarrier { depth } => depth,
            Self::Suppressed => 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AmParams {
    pub mode: AmMode,
    pub bandwidth: f64,
    pub vestige: Option<f64>,
    pub band_taps: usize,
    pub audio_taps: usize,
}

impl AmParams {
    #[must_use]
    pub fn new(mode: AmMode, bandwidth: f64) -> Self {
        Self {
            mode,
            bandwidth,
            vestige: None,
            band_taps: DEFAULT_TAPS,
            audio_taps: DEFAULT_TAPS,
        }
    }

    #[must_use]
    pub fn band(&self) -> BandFilter {
        match self.vestige {
            None => BandFilter::symmetric(self.bandwidth, self.band_taps),
            Some(vestige) => BandFilter {
                low: -vestige,
                high: self.bandwidth,
                taps: self.band_taps,
            },
        }
    }
}

#[cfg(test)]
mod tests;

mod demod;
mod modulator;

pub use self::{
    demod::{AngleDemod, AngleDetector, AngleRx},
    modulator::AngleMod,
};
use super::{filter::BandFilter, stage::DEFAULT_TAPS};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AngleKind {
    Fm { deviation: f64 },
    Pm { deviation_rad: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AngleParams {
    pub kind: AngleKind,
    pub bandwidth: f64,
    pub band_taps: usize,
    pub audio_taps: usize,
}

impl AngleParams {
    #[must_use]
    pub fn new(kind: AngleKind, bandwidth: f64) -> Self {
        Self {
            kind,
            bandwidth,
            band_taps: DEFAULT_TAPS,
            audio_taps: DEFAULT_TAPS,
        }
    }

    #[must_use]
    pub fn deviation_ratio(&self) -> f64 {
        match self.kind {
            AngleKind::Fm { deviation } => deviation / self.bandwidth,
            AngleKind::Pm { deviation_rad } => deviation_rad,
        }
    }

    #[must_use]
    pub fn carson_bandwidth(&self) -> f64 {
        2.0 * (self.deviation_ratio() + 1.0) * self.bandwidth
    }

    #[must_use]
    pub fn band(&self) -> BandFilter {
        BandFilter::symmetric(0.5 * self.carson_bandwidth(), self.band_taps)
    }
}

#[cfg(test)]
mod tests;

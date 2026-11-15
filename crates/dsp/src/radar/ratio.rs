use super::RadarDspError;
use crate::math::db::Decibels;

pub const HEALTH_WINDOW_S: f64 = 0.5;

const RATIO_FLOOR: f64 = 1e-12;

/// Ratio of two energies summed over a fixed time window, kept from the last full window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WindowRatio {
    window: usize,
    count: usize,
    numerator: f64,
    denominator: f64,
    last_db: Option<f32>,
    last_denominator_mean: Option<f64>,
}

impl WindowRatio {
    pub(crate) fn new(sample_rate: f64) -> Result<Self, RadarDspError> {
        if !(sample_rate.is_finite() && sample_rate > 0.0) {
            return Err(RadarDspError::Setting);
        }
        Ok(Self {
            window: ((HEALTH_WINDOW_S * sample_rate).round() as usize).max(1),
            count: 0,
            numerator: 0.0,
            denominator: 0.0,
            last_db: None,
            last_denominator_mean: None,
        })
    }

    pub(crate) fn add(&mut self, numerator: f32, denominator: f32, samples: usize) {
        self.numerator += f64::from(numerator);
        self.denominator += f64::from(denominator);
        self.count += samples;
        if self.count >= self.window {
            self.last_db = Some(ratio_db(self.numerator, self.denominator));
            self.last_denominator_mean = Some(self.denominator / self.count as f64);
            self.count = 0;
            self.numerator = 0.0;
            self.denominator = 0.0;
        }
    }

    pub(crate) fn value_db(&self) -> f32 {
        self.last_db
            .unwrap_or_else(|| ratio_db(self.numerator, self.denominator))
    }

    pub(crate) fn denominator_mean(&self) -> Option<f64> {
        self.last_denominator_mean
    }

    pub(crate) fn clear(&mut self) {
        self.count = 0;
        self.numerator = 0.0;
        self.denominator = 0.0;
        self.last_db = None;
        self.last_denominator_mean = None;
    }
}

/// Energy ratio in dB, zero when either energy is unusable, capped where the denominator
/// falls below a trillionth of the numerator.
pub(crate) fn ratio_db(numerator: f64, denominator: f64) -> f32 {
    if !(numerator > 0.0 && numerator.is_finite() && denominator.is_finite()) {
        return 0.0;
    }
    (numerator / denominator.max(RATIO_FLOOR * numerator)).power_to_db() as f32
}

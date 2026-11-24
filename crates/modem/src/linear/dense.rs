mod line_timing;
#[cfg(test)]
mod tests;
mod tracker;

use std::fmt;

use num_complex::Complex;
use sdrmm_dsp::Decimator;

use super::acquire::FrequencyAcquisition;
use crate::{constellation::Constellation, pulse::assert_unit_energy};
use line_timing::{LineTiming, WARM_SYMBOLS};
pub use tracker::DenseState;
use tracker::TrustTracker;

/// Equaliser taps that cover an echo a few symbols out without costing much per symbol.
pub const DEFAULT_EQUALISER_TAPS: usize = 11;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DenseError {
    NotSquareGrid,
    TooFewSamplesPerSymbol,
}

impl fmt::Display for DenseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSquareGrid => write!(f, "dense demodulation needs a square QAM grid"),
            Self::TooFewSamplesPerSymbol => write!(f, "dense demodulation needs at least 4 sps"),
        }
    }
}

impl std::error::Error for DenseError {}

/// A continuous receiver for square QAM too dense for decision loops to start on: 65536 points
/// and past. Timing comes from the square-law line, gain and carrier from a blind start handed
/// to decision-directed loops that trust only the points they already decide right, with a short
/// equaliser for echoes. Output symbols are at unit mean power with the grid's quarter-turn
/// ambiguity left for a preamble to settle.
pub struct DenseQamDemod {
    matched: Decimator,
    filtered: Vec<Complex<f32>>,
    timing: LineTiming,
    tracker: TrustTracker,
    frequency: FrequencyAcquisition,
    opening: Vec<Complex<f32>>,
    opened: bool,
}

impl DenseQamDemod {
    /// `receive_filter` is the unit-energy matched filter at `sps` samples per symbol, which need
    /// not be whole.
    pub fn new(
        table: Constellation,
        receive_filter: &[f32],
        sps: f64,
        equaliser_taps: usize,
    ) -> Result<Self, DenseError> {
        if !table.is_square_grid() {
            return Err(DenseError::NotSquareGrid);
        }
        if !(sps >= 4.0 && sps.is_finite()) {
            return Err(DenseError::TooFewSamplesPerSymbol);
        }
        assert_unit_energy(receive_filter);
        Ok(Self {
            matched: Decimator::new(receive_filter, 1),
            filtered: Vec::new(),
            timing: LineTiming::new(sps),
            tracker: TrustTracker::new(table, equaliser_taps),
            frequency: FrequencyAcquisition::new(),
            opening: Vec::with_capacity(WARM_SYMBOLS),
            opened: false,
        })
    }

    pub fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        self.matched.process(iq, &mut self.filtered);
        let Self {
            timing,
            tracker,
            frequency,
            opening,
            opened,
            filtered,
            ..
        } = self;
        timing.process(filtered, |y| {
            if *opened {
                out.push(tracker.push(y));
                return;
            }
            opening.push(y);
            if opening.len() < WARM_SYMBOLS {
                return;
            }
            if let Some(chirp) = frequency.estimate(opening, 4) {
                tracker.seed_frequency(chirp.centre_cycles_per_symbol);
            }
            out.extend(opening.iter().map(|&s| tracker.push(s)));
            opening.clear();
            *opened = true;
        });
    }

    #[must_use]
    pub fn state(&self) -> DenseState {
        self.tracker.state()
    }

    pub fn reset(&mut self) {
        self.timing.reset();
        self.tracker.reset();
        self.opening.clear();
        self.opened = false;
    }
}

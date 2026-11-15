mod boxcar;
mod clocks;
#[cfg(test)]
mod tests;

use num_complex::Complex;

use super::{cfo::SpectralShift, derotate::derotate};
use crate::{math::db::Decibels, math::fft::FftPair};

pub use boxcar::Boxcar;

pub const COARSE_FRAME: usize = 32_768;
pub const COARSE_LAGS: usize = 16_384;
pub const COARSE_PEAK_DB: f32 = 15.0;
pub const COARSE_SECOND_DB: f32 = 6.0;
pub const MAX_CFO_HZ: f64 = 5_000.0;

const REACH_S: f64 = 0.25;
const MAX_DECIMATION: usize = 1_024;
const SPECTRUM_BINS: usize = 1_024;
const SEGMENT: usize = 1_024;
const PEAK_GUARD: usize = 2;

#[must_use]
pub fn coarse_decimation(sample_rate: f64, lags: usize) -> usize {
    if !(sample_rate.is_finite() && sample_rate > 0.0) || lags == 0 {
        return 1;
    }
    let wanted = (REACH_S * sample_rate / lags as f64).ceil();
    if wanted >= MAX_DECIMATION as f64 {
        return MAX_DECIMATION;
    }
    (wanted.max(1.0) as usize)
        .next_power_of_two()
        .min(MAX_DECIMATION)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoarseLag {
    pub lag: i64,
    pub peak_db: f32,
    pub second_db: f32,
    pub cfo_hz: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CoarseError {
    #[error("no correlation peak")]
    NoPeak,
    #[error("ambiguous correlation")]
    Ambiguous,
    #[error("capture too short")]
    Short,
    #[error("sample rate out of range")]
    Rate,
    #[error("expected {expected} lanes, got {got}")]
    LaneCount { expected: usize, got: usize },
    #[error("lanes differ in length")]
    LaneLength,
}

pub struct CoarseSearch {
    fft: FftPair,
    frame: usize,
    lags: usize,
    a: Vec<Complex<f32>>,
    b: Vec<Complex<f32>>,
    lane_spectrum: Vec<Complex<f32>>,
    power: Vec<f64>,
    shift: SpectralShift,
}

impl CoarseSearch {
    #[must_use]
    pub fn new(frame: usize, lags: usize) -> Self {
        let frame = frame.max(1);
        let size = (frame + 2 * lags).next_power_of_two().max(2);
        Self {
            fft: FftPair::new(size),
            frame,
            lags,
            a: vec![Complex::default(); size],
            b: vec![Complex::default(); size],
            lane_spectrum: vec![Complex::default(); size],
            power: vec![0.0; 2 * lags + 1],
            shift: SpectralShift::new(SPECTRUM_BINS),
        }
    }

    #[must_use]
    pub const fn span(&self) -> usize {
        self.frame + 2 * self.lags
    }

    pub fn lag(
        &mut self,
        reference: &[Complex<f32>],
        lane: &[Complex<f32>],
    ) -> Result<CoarseLag, CoarseError> {
        self.check(reference, lane)?;
        self.coherent(reference, lane, 0.0)
    }

    fn check(&self, reference: &[Complex<f32>], lane: &[Complex<f32>]) -> Result<(), CoarseError> {
        if reference.len() < self.span() || lane.len() < self.span() {
            return Err(CoarseError::Short);
        }
        Ok(())
    }

    fn load_reference(&mut self, reference: &[Complex<f32>], start: usize, end: usize) {
        self.a.fill(Complex::default());
        self.a[start..end].copy_from_slice(&reference[self.lags + start..self.lags + end]);
        self.fft.forward(&mut self.a);
    }

    fn coherent(
        &mut self,
        reference: &[Complex<f32>],
        lane: &[Complex<f32>],
        cycles: f64,
    ) -> Result<CoarseLag, CoarseError> {
        let span = self.span();
        self.load_reference(reference, 0, self.frame);
        self.b.fill(Complex::default());
        derotate(&lane[..span], 0, cycles, &mut self.b[..span]);
        self.fft.forward(&mut self.b);
        for (value, reference) in self.b.iter_mut().zip(&self.a) {
            *value *= reference.conj();
        }
        self.fft.inverse(&mut self.b);
        for (power, value) in self.power.iter_mut().zip(&self.b) {
            *power = f64::from(value.norm_sqr());
        }
        self.judge()
    }

    fn judge(&self) -> Result<CoarseLag, CoarseError> {
        let (peak, best) = self
            .power
            .iter()
            .copied()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .ok_or(CoarseError::NoPeak)?;
        let mean = self.power.iter().sum::<f64>() / self.power.len() as f64;
        if !(best.is_finite() && mean.is_finite()) || best <= 0.0 || mean <= 0.0 {
            return Err(CoarseError::NoPeak);
        }
        let second = self
            .power
            .iter()
            .enumerate()
            .filter(|(index, _)| index.abs_diff(peak) > PEAK_GUARD)
            .map(|(_, power)| *power)
            .fold(0.0, f64::max);
        let peak_db = (best / mean).power_to_db() as f32;
        let second_db = if second > 0.0 {
            (best / second).power_to_db() as f32
        } else {
            f32::INFINITY
        };
        if peak_db.is_nan() || peak_db < COARSE_PEAK_DB {
            return Err(CoarseError::NoPeak);
        }
        if second_db < COARSE_SECOND_DB {
            return Err(CoarseError::Ambiguous);
        }
        Ok(CoarseLag {
            lag: peak as i64 - self.lags as i64,
            peak_db,
            second_db,
            cfo_hz: 0.0,
        })
    }
}

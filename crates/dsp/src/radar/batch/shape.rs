use num_complex::Complex;

use super::{MAX_FFT, MAX_SURVEILLANCE, MIN_FFT};
use crate::{math::complex::sum_norm_sqr, radar::RadarDspError};

type C32 = Complex<f32>;

/// Samples summed in f32 by SIMD before the running total moves to f64, so long batches keep
/// their precision.
const ENERGY_BLOCK: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchShape {
    pub batches: usize,
    pub batch_len: usize,
    pub gates: usize,
    pub lead: usize,
    pub taps: usize,
    pub lanes: usize,
    pub fft_len: usize,
}

impl BatchShape {
    pub fn new(
        batches: usize,
        batch_len: usize,
        gates: usize,
        lead: usize,
        taps: usize,
        lanes: usize,
    ) -> Result<Self, RadarDspError> {
        let empty = batches == 0 || batch_len == 0 || gates == 0;
        if empty || !(1..=MAX_SURVEILLANCE).contains(&lanes) || (lead > 0 && taps == 0) {
            return Err(RadarDspError::Shape);
        }
        let mut shape = Self {
            batches,
            batch_len,
            gates,
            lead,
            taps,
            lanes,
            fft_len: 0,
        };
        let reach = if shape.eca() {
            batch_len + shape.span() + shape.order() - 2
        } else {
            batch_len + shape.span() - 1
        };
        let fft_len = reach
            .checked_next_power_of_two()
            .ok_or(RadarDspError::Size)?
            .max(MIN_FFT);
        if fft_len > MAX_FFT {
            return Err(RadarDspError::Size);
        }
        shape.fft_len = fft_len;
        Ok(shape)
    }

    #[must_use]
    pub const fn order(&self) -> usize {
        self.lead + self.taps
    }

    #[must_use]
    pub const fn eca(&self) -> bool {
        self.order() > 0
    }

    #[must_use]
    pub const fn span(&self) -> usize {
        if self.eca() {
            let reach = if self.gates > self.taps {
                self.gates
            } else {
                self.taps
            };
            reach + self.lead
        } else {
            self.gates
        }
    }

    #[must_use]
    pub const fn pre(&self) -> usize {
        self.order().saturating_sub(1)
    }

    #[must_use]
    pub const fn post(&self) -> usize {
        self.span() - 1
    }

    #[must_use]
    pub const fn samples(&self) -> usize {
        self.batches * self.batch_len
    }

    #[must_use]
    pub const fn window(&self) -> usize {
        self.samples() + self.pre() + self.post()
    }

    #[must_use]
    pub fn energy(&self, window: &[C32], batch: usize) -> f64 {
        let start = self.pre() + batch * self.batch_len;
        window
            .get(start..start + self.batch_len)
            .map_or(0.0, |samples| {
                samples
                    .chunks(ENERGY_BLOCK)
                    .map(|block| f64::from(sum_norm_sqr(block)))
                    .sum()
            })
    }
}

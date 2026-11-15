use num_complex::Complex;

use super::BinSolver;
use crate::{
    array::array_sync::{EQ_POINTS, equaliser::clip},
    math::complex::{narrow, widen},
};

const EQ_SMOOTH_BINS: usize = 9;

impl BinSolver {
    pub(super) fn equaliser(
        &mut self,
        lane: usize,
        phase: f64,
        delay: f64,
        gain: f64,
    ) -> Vec<Complex<f32>> {
        let order = self.lanes;
        let one = Complex::new(1.0, 0.0);
        for centred in 0..self.len {
            let bin = self.bin_of(centred);
            self.residual[centred] = if self.fits[bin].used && gain > 0.0 {
                let model = self.fitted(bin, gain, phase, delay);
                narrow(widen(self.responses[bin * order + lane]) / model)
            } else {
                one
            };
        }
        let reach = EQ_SMOOTH_BINS / 2;
        for centred in 0..self.len {
            self.smoothed[centred] = if self.is_used(centred) {
                clip(self.used_mean(&self.residual, centred, reach))
            } else {
                one
            };
        }
        let spread = self.len / EQ_POINTS / 2;
        (0..EQ_POINTS)
            .map(|point| {
                let centre = ((point as f64 / EQ_POINTS as f64) * self.len as f64).round() as usize;
                let centre = centre.min(self.len - 1);
                if (centre.saturating_sub(spread)..=(centre + spread).min(self.len - 1))
                    .any(|centred| self.is_used(centred))
                {
                    clip(self.used_mean(&self.smoothed, centre, spread))
                } else {
                    one
                }
            })
            .collect()
    }

    fn used_mean(&self, values: &[Complex<f32>], centre: usize, reach: usize) -> Complex<f32> {
        let mut sum = Complex::<f32>::default();
        let mut count = 0usize;
        let first = centre.saturating_sub(reach);
        let last = (centre + reach).min(self.len - 1);
        for (centred, value) in values.iter().enumerate().take(last + 1).skip(first) {
            if self.is_used(centred) {
                sum += value;
                count += 1;
            }
        }
        if count == 0 {
            Complex::new(1.0, 0.0)
        } else {
            sum / count as f32
        }
    }
}

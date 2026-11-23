use num_complex::Complex;
use sdrmm_dsp::math::complex::widen;

use super::LineFit;

pub const TRACK_ALPHA: f64 = 0.35;

pub const TRACK_BETA: f64 = TRACK_ALPHA * TRACK_ALPHA / (2.0 - TRACK_ALPHA);

/// Pilot offsets closer than this to one point fit a common phase and no slope.
const DEGENERATE_SPREAD: f64 = 1e-12;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PilotFit {
    pub common_rad: f64,
    pub slope_rad_per_bin: f64,
}

impl PilotFit {
    #[must_use]
    pub fn phase_at(&self, offset: i32) -> f64 {
        self.common_rad + self.slope_rad_per_bin * f64::from(offset)
    }
}

#[derive(Clone, Debug, Default)]
pub struct PilotTracker {
    state: PilotFit,
    rate: PilotFit,
    symbols: usize,
}

impl PilotTracker {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.state = PilotFit::default();
        self.rate = PilotFit::default();
        self.symbols = 0;
    }

    pub fn fit(&mut self, offsets: &[i32], errors: &[Complex<f32>], weights: &[f32]) -> PilotFit {
        let predicted = PilotFit {
            common_rad: self.state.common_rad + self.rate.common_rad,
            slope_rad_per_bin: self.state.slope_rad_per_bin + self.rate.slope_rad_per_bin,
        };
        let Some(measured) = weighted_line(offsets, errors, weights, predicted) else {
            self.state = predicted;
            self.symbols += 1;
            return predicted;
        };
        let fit = if self.symbols == 0 {
            self.rate = PilotFit::default();
            measured
        } else {
            let residual = PilotFit {
                common_rad: measured.common_rad - predicted.common_rad,
                slope_rad_per_bin: measured.slope_rad_per_bin - predicted.slope_rad_per_bin,
            };
            self.rate = PilotFit {
                common_rad: self.rate.common_rad + TRACK_BETA * residual.common_rad,
                slope_rad_per_bin: self.rate.slope_rad_per_bin
                    + TRACK_BETA * residual.slope_rad_per_bin,
            };
            PilotFit {
                common_rad: predicted.common_rad + TRACK_ALPHA * residual.common_rad,
                slope_rad_per_bin: predicted.slope_rad_per_bin
                    + TRACK_ALPHA * residual.slope_rad_per_bin,
            }
        };
        self.state = fit;
        self.symbols += 1;
        fit
    }

    #[must_use]
    pub fn last(&self) -> PilotFit {
        self.state
    }

    #[must_use]
    pub fn symbols(&self) -> usize {
        self.symbols
    }
}

fn weighted_line(
    offsets: &[i32],
    errors: &[Complex<f32>],
    weights: &[f32],
    predicted: PilotFit,
) -> Option<PilotFit> {
    let mut fit = LineFit::default();
    for ((&offset, &error), &weight) in offsets.iter().zip(errors).zip(weights) {
        let w = f64::from(weight);
        if w <= 0.0 {
            continue;
        }
        let at = predicted.phase_at(offset);
        let residual = widen(error) * Complex::from_polar(1.0, -at);
        fit.push(f64::from(offset), at + residual.arg(), w);
    }
    if fit.weight() <= 0.0 {
        return None;
    }
    let (common_rad, slope_rad_per_bin) = fit.line(DEGENERATE_SPREAD);
    Some(PilotFit {
        common_rad,
        slope_rad_per_bin,
    })
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{PI, TAU};

    use sdrmm_dsp::math::special::wrap_rad;

    use super::*;

    #[test]
    fn the_pilot_fit_recovers_a_line_and_ignores_dead_pilots() {
        let offsets = [-21i32, -7, 7, 21];
        let truth = PilotFit {
            common_rad: 0.3,
            slope_rad_per_bin: 0.01,
        };
        let errors: Vec<Complex<f32>> = offsets
            .iter()
            .map(|&k| {
                let p = truth.phase_at(k);
                Complex::new(p.cos() as f32, p.sin() as f32)
            })
            .collect();
        let mut tracker = PilotTracker::new();
        let fit = tracker.fit(&offsets, &errors, &[1.0; 4]);
        assert!((fit.common_rad - truth.common_rad).abs() < 1e-6, "{fit:?}");
        assert!((fit.slope_rad_per_bin - truth.slope_rad_per_bin).abs() < 1e-9);

        let mut spoiled = errors.clone();
        spoiled[0] = Complex::new(-1.0, 0.0);
        tracker.reset();
        let fit = tracker.fit(&offsets, &spoiled, &[0.0, 1.0, 1.0, 1.0]);
        assert!((fit.common_rad - truth.common_rad).abs() < 1e-6, "{fit:?}");
    }

    #[test]
    fn tracking_follows_a_slope_past_the_unwrapping_ambiguity() {
        let pilots = [-21i32, -7, 7, 21];
        let step = TAU * 0.24 / 64.0;
        let mut tracked = PilotTracker::new();
        let mut untracked = PilotTracker::new();
        let (mut transient, mut steady, mut worst_untracked) = (0.0f64, 0.0f64, 0.0f64);
        for symbol in 0..64 {
            let truth = PilotFit {
                common_rad: 0.0,
                slope_rad_per_bin: step * f64::from(symbol),
            };
            let errors: Vec<Complex<f32>> = pilots
                .iter()
                .map(|&k| {
                    let p = truth.phase_at(k);
                    Complex::new(p.cos() as f32, p.sin() as f32)
                })
                .collect();
            let a = tracked.fit(&pilots, &errors, &[1.0; 4]);
            untracked.reset();
            let b = untracked.fit(&pilots, &errors, &[1.0; 4]);
            let residual = |fit: PilotFit| {
                (-26..=26)
                    .map(|k| wrap_rad(fit.phase_at(k) - truth.phase_at(k)).abs())
                    .fold(0.0f64, f64::max)
            };
            transient = transient.max(residual(a));
            if symbol >= 32 {
                steady = steady.max(residual(a));
            }
            worst_untracked = worst_untracked.max(residual(b));
        }
        assert!(transient < PI / 2.0, "transient residual {transient} rad");
        assert!(steady < 0.02, "steady-state residual {steady} rad");
        assert_eq!(tracked.symbols(), 64);
        assert!(
            worst_untracked > 1.0,
            "untracked residual {worst_untracked} rad: the ambiguity was never reached, so \
             this test is not measuring what it claims"
        );
    }
}

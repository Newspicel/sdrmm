use std::f64::consts::PI;

use num_complex::Complex;

use crate::linear::LinearParams;

const TIMING_MISFIT_SYMBOLS: f32 = 64.0;

const TRUST_MISFIT: f32 = 0.08;

const DISTRUST_MISFIT: f32 = 0.11;

/// A Mueller and Muller timing error that only steers once decisions are good enough to trust.
pub(super) struct DecisionTiming {
    previous: Complex<f32>,
    previous_decision: Complex<f32>,
    misfit: f32,
    trusted: bool,
    spacing: f32,
}

impl DecisionTiming {
    pub(super) fn new(params: &LinearParams) -> Self {
        Self {
            previous: Complex::new(0.0, 0.0),
            previous_decision: Complex::new(0.0, 0.0),
            misfit: 1.0,
            trusted: false,
            spacing: params.constellation().min_distance().max(f64::MIN_POSITIVE) as f32,
        }
    }

    pub(super) fn error(&mut self, z: Complex<f32>, decision: Complex<f32>) -> Option<f64> {
        let miss = (z - decision).norm_sqr() / (self.spacing * self.spacing);
        self.misfit += (miss - self.misfit) / TIMING_MISFIT_SYMBOLS;
        self.trusted = if self.trusted {
            self.misfit < DISTRUST_MISFIT
        } else {
            self.misfit < TRUST_MISFIT
        };
        let raw = (self.previous_decision.conj() * z - decision.conj() * self.previous).re;
        self.previous = z;
        self.previous_decision = decision;
        self.trusted
            .then(|| -f64::from(raw) / PI)
            .filter(|e| e.is_finite())
    }

    pub(super) fn reset(&mut self) {
        self.previous = Complex::new(0.0, 0.0);
        self.previous_decision = Complex::new(0.0, 0.0);
        self.misfit = 1.0;
        self.trusted = false;
    }
}

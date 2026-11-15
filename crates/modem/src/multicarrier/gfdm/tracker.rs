use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::math::complex::widen;

use crate::constellation::Constellation;

pub const FREQ_GAIN: f64 = 1.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarrierTracker {
    phase: f64,
    cfo: f64,
}

impl CarrierTracker {
    #[must_use]
    pub fn new(cfo: f64, first_sample: usize) -> Self {
        Self {
            phase: (TAU * cfo * first_sample as f64).rem_euclid(TAU),
            cfo,
        }
    }

    #[must_use]
    pub fn phase(&self) -> f64 {
        self.phase
    }

    #[must_use]
    pub fn cfo(&self) -> f64 {
        self.cfo
    }

    pub fn correct(
        &mut self,
        points: &mut [Complex<f32>],
        table: &Constellation,
        window: usize,
        span: usize,
    ) -> f64 {
        let error = decision_phase(points, table);
        rotate(points, -error);
        let centre = window as f64 / 2.0;
        let span = span as f64;
        let before = self.cfo;
        self.cfo += FREQ_GAIN * error / (TAU * span);
        let advance = TAU * (before * centre + self.cfo * (span - centre));
        self.phase = (self.phase + error + advance).rem_euclid(TAU);
        error
    }
}

fn rotate(points: &mut [Complex<f32>], rad: f64) {
    let turn = Complex::from_polar(1.0f32, rad as f32);
    for p in points {
        *p *= turn;
    }
}

fn decision_phase(points: &[Complex<f32>], table: &Constellation) -> f64 {
    let mut acc = Complex::new(0.0f64, 0.0);
    for &p in points {
        let d = table.nearest(p);
        let v = p * d.conj();
        acc += widen(v);
    }
    acc.arg()
}

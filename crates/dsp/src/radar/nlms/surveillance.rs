use num_complex::Complex;

use super::delay_line::DelayLine;
use super::{REGRESSOR_FLOOR, check_canceller};
use crate::radar::RadarDspError;
use crate::radar::ratio::WindowRatio;
use crate::radar::regressor::{Regressor, filter};

type C32 = Complex<f32>;

/// Sample-by-sample NLMS that cancels the reference out of one surveillance lane.
pub struct SurveillanceNlms {
    order: usize,
    lead: usize,
    step: f32,
    weights: Vec<C32>,
    regressor: Regressor,
    delay: DelayLine,
    meter: WindowRatio,
    resets: u64,
}

impl SurveillanceNlms {
    pub fn new(
        taps: usize,
        lead: usize,
        step: f32,
        sample_rate: f64,
    ) -> Result<Self, RadarDspError> {
        let order = check_canceller(taps, lead, step)?;
        Ok(Self {
            order,
            lead,
            step,
            weights: vec![C32::default(); order],
            regressor: Regressor::new(order),
            delay: DelayLine::new(lead),
            meter: WindowRatio::new(sample_rate)?,
            resets: 0,
        })
    }

    #[must_use]
    pub const fn latency(&self) -> usize {
        self.lead
    }

    #[must_use]
    pub const fn order(&self) -> usize {
        self.order
    }

    pub fn process(&mut self, reference: &[C32], surveillance: &[C32], out: &mut [C32]) -> usize {
        let len = reference.len().min(surveillance.len()).min(out.len());
        for ((&r, &s), e) in reference
            .iter()
            .zip(surveillance)
            .zip(out.iter_mut())
            .take(len)
        {
            *e = self.step_sample(r, s);
        }
        len
    }

    #[must_use]
    pub fn suppression_db(&self) -> f32 {
        self.meter.value_db()
    }

    #[must_use]
    pub const fn resets(&self) -> u64 {
        self.resets
    }

    #[must_use]
    pub fn weights(&self) -> &[C32] {
        &self.weights
    }

    pub fn reset(&mut self) {
        self.weights.fill(C32::default());
        self.regressor.clear();
        self.delay.clear();
        self.meter.clear();
    }

    fn step_sample(&mut self, reference: C32, surveillance: C32) -> C32 {
        self.regressor.push(reference);
        let desired = self.delay.push(surveillance);
        let regressor = self.regressor.window();
        let (estimate, norm) = filter(&self.weights, regressor);
        let error = desired - estimate;
        let gain = error.conj() * (self.step / (REGRESSOR_FLOOR + norm));
        let mut energy = 0.0f32;
        for (w, x) in self.weights.iter_mut().zip(regressor) {
            *w += gain * x;
            energy += w.norm_sqr();
        }
        if !(energy.is_finite() && error.is_finite()) {
            self.weights.fill(C32::default());
            self.regressor.clear();
            self.meter.clear();
            self.resets += 1;
            return if desired.is_finite() {
                desired
            } else {
                C32::default()
            };
        }
        self.meter.add(desired.norm_sqr(), error.norm_sqr(), 1);
        error
    }
}

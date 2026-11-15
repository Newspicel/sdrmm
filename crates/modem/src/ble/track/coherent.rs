use num_complex::Complex;

use super::{super::SPS, HALF_SAMPLE, MAX_TIMING_STEP, QUARTER, View};

const CARRIER_MEMORY: f32 = 0.85;
const SYMBOL_TIMING_GAIN: f64 = 0.02;

pub(crate) struct Coherent<'a> {
    view: View<'a>,
    position: f64,
    carrier: Complex<f32>,
    previous: Complex<f32>,
}

impl<'a> Coherent<'a> {
    pub(crate) fn new(
        view: View<'a>,
        position: f64,
        carrier: Complex<f32>,
        previous: Complex<f32>,
    ) -> Self {
        Self {
            view,
            position,
            carrier,
            previous,
        }
    }

    pub(crate) fn position(&self) -> f64 {
        self.position
    }

    pub(crate) fn symbol(&mut self) -> Option<f32> {
        self.position += SPS as f64;
        let y = self.view.at(self.position)?;
        let scale = self.carrier.norm().max(f32::MIN_POSITIVE);
        let expected = self.carrier * self.previous;
        let soft = (y * expected.conj()).im / scale;
        let current = self.previous * if soft > 0.0 { QUARTER } else { -QUARTER };
        let reference = self.carrier * current;
        let slope = self.view.at(self.position + HALF_SAMPLE)?
            - self.view.at(self.position - HALF_SAMPLE)?;
        let error = f64::from((slope * reference.conj()).re / (scale * scale));
        self.position += SYMBOL_TIMING_GAIN * error.clamp(-MAX_TIMING_STEP, MAX_TIMING_STEP);
        self.carrier = self.carrier * CARRIER_MEMORY + y * current.conj() * (1.0 - CARRIER_MEMORY);
        self.previous = current;
        Some(soft)
    }
}

use num_complex::Complex;

use super::{
    super::{SPS, coded::S8_SYMBOLS},
    HALF_SAMPLE, MAX_TIMING_STEP, View,
};

const BLOCK_TIMING_GAIN: f64 = 0.08;
const PILOT_MEMORY: f32 = 0.8;
const SMOOTHING_BLOCKS: usize = 8;
const BLOCK: f64 = (SPS * S8_SYMBOLS) as f64;

pub(crate) struct Blocks<'a> {
    view: View<'a>,
    position: f64,
    reference: Complex<f32>,
}

impl<'a> Blocks<'a> {
    pub(crate) fn new(view: View<'a>, position: f64, reference: Complex<f32>) -> Self {
        Self {
            view,
            position,
            reference,
        }
    }

    pub(crate) fn position(&self) -> f64 {
        self.position
    }

    pub(crate) fn reference(&self) -> Complex<f32> {
        self.reference
    }

    pub(crate) fn block(&mut self) -> Option<(Complex<f32>, Complex<f32>)> {
        let t = self.position;
        let step = SPS as f64;
        let [y0, y1, y2, y3] = [0.0, 1.0, 2.0, 3.0].map(|k| self.view.at(t + k * step));
        let (y0, y1, y2, y3) = (y0?, y1?, y2?, y3?);
        let pilot = y0 - y2;
        let data = y1 + y3;
        let slope = |at: f64| -> Option<Complex<f32>> {
            Some(self.view.at(at + HALF_SAMPLE)? - self.view.at(at - HALF_SAMPLE)?)
        };
        let derivative = slope(t)? - slope(t + 2.0 * step)?;
        let scale = self.reference.norm_sqr().max(f32::MIN_POSITIVE);
        let error = f64::from((derivative * self.reference.conj()).re / scale);
        self.reference = self.reference * PILOT_MEMORY + pilot * (1.0 - PILOT_MEMORY);
        self.position += BLOCK + BLOCK_TIMING_GAIN * error.clamp(-MAX_TIMING_STEP, MAX_TIMING_STEP);
        Some((pilot, data))
    }
}

pub(crate) fn smoothed_soft(pilots: &[Complex<f32>], data: &[Complex<f32>], out: &mut Vec<f32>) {
    for (index, &value) in data.iter().enumerate() {
        let low = index.saturating_sub(SMOOTHING_BLOCKS);
        let high = (index + SMOOTHING_BLOCKS + 1).min(pilots.len());
        let window = &pilots[low..high];
        let drift: Complex<f32> = window.windows(2).map(|pair| pair[1] * pair[0].conj()).sum();
        let turn = if drift.norm() > 0.0 { drift.arg() } else { 0.0 };
        let reference: Complex<f32> = window
            .iter()
            .enumerate()
            .map(|(offset, &pilot)| {
                let distance = (low + offset) as f32 - index as f32;
                pilot * Complex::from_polar(1.0, -turn * distance)
            })
            .sum();
        let scale = reference.norm().max(f32::MIN_POSITIVE);
        out.push((value * reference.conj()).im / scale);
    }
}

pub(crate) fn carrier_from_pilot(reference: Complex<f32>, last: Complex<f32>) -> Complex<f32> {
    reference * last.conj() * 0.5
}

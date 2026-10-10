mod burst;
mod floor;

use num_complex::Complex;

pub use self::{
    burst::{Burst, Bursts},
    floor::QuietFloor,
};
use crate::{fft::Transform, window::hann};

pub const HANN_BIN_SPREAD: f32 = 1.5;

pub struct PowerFrames {
    fft: Transform,
    window: Vec<f32>,
    buf: Vec<Complex<f32>>,
    power: Vec<f32>,
    fill: usize,
    scale: f32,
}

impl PowerFrames {
    #[must_use]
    pub fn new(len: usize) -> Self {
        let len = len.max(2);
        let window = hann(len);
        let energy: f32 = window.iter().map(|w| w * w).sum();
        Self {
            fft: Transform::forward(len),
            buf: vec![Complex::new(0.0, 0.0); len],
            power: vec![0.0; len],
            fill: 0,
            scale: 1.0 / (len as f32 * energy),
            window,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.window.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.window.is_empty()
    }

    pub fn reset(&mut self) {
        self.fill = 0;
    }

    pub fn process(&mut self, iq: &[Complex<f32>], mut frame: impl FnMut(&[f32])) {
        let len = self.window.len();
        let mut rest = iq;
        while !rest.is_empty() {
            let take = (len - self.fill).min(rest.len());
            let (head, tail) = rest.split_at(take);
            for ((slot, &sample), &weight) in self.buf[self.fill..]
                .iter_mut()
                .zip(head)
                .zip(&self.window[self.fill..])
            {
                *slot = sample * weight;
            }
            self.fill += take;
            rest = tail;
            if self.fill == len {
                self.fill = 0;
                self.transform();
                frame(&self.power);
            }
        }
    }

    fn transform(&mut self) {
        self.fft.process(&mut self.buf);
        let half = self.buf.len() / 2;
        let (positive, negative) = self.buf.split_at(self.buf.len() - half);
        let (low, high) = self.power.split_at_mut(half);
        for (slot, bin) in low.iter_mut().zip(negative) {
            *slot = bin.norm_sqr() * self.scale;
        }
        for (slot, bin) in high.iter_mut().zip(positive) {
            *slot = bin.norm_sqr() * self.scale;
        }
    }
}

#[must_use]
pub fn band_degrees(bins: usize) -> f32 {
    2.0 * (bins as f32 / HANN_BIN_SPREAD).max(1.0)
}

#[cfg(test)]
mod tests;

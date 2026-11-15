use num_complex::Complex;

use super::ZERO;

/// A newest-first window kept twice over, so the taps always read one contiguous slice.
#[derive(Clone, Debug)]
pub(super) struct DelayLine {
    samples: Vec<Complex<f32>>,
    head: usize,
    len: usize,
}

impl DelayLine {
    pub(super) fn new(len: usize) -> Self {
        Self {
            samples: vec![ZERO; 2 * len],
            head: 0,
            len,
        }
    }

    pub(super) fn push(&mut self, x: Complex<f32>) {
        if self.len == 0 {
            return;
        }
        self.head = (self.head + self.len - 1) % self.len;
        self.samples[self.head] = x;
        self.samples[self.head + self.len] = x;
    }

    pub(super) fn window(&self) -> &[Complex<f32>] {
        &self.samples[self.head..self.head + self.len]
    }

    pub(super) fn clear(&mut self) {
        self.samples.fill(ZERO);
        self.head = 0;
    }
}

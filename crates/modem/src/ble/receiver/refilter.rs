use num_complex::Complex;
use sdrmm_dsp::Decimator;

use super::MAX_HISTORY;

pub(super) struct Refilter {
    filter: Decimator,
    rotated: Vec<Complex<f32>>,
    pub(super) out: Vec<Complex<f32>>,
}

impl Refilter {
    pub(super) fn new(pulse: &[f32]) -> Self {
        Self {
            filter: Decimator::new(pulse, 1),
            rotated: Vec::with_capacity(MAX_HISTORY),
            out: Vec::with_capacity(MAX_HISTORY),
        }
    }

    pub(super) fn run(&mut self, raw: &[Complex<f32>], omega: f32, origin: usize) {
        self.rotated.clear();
        let step = Complex::from_polar(1.0, -omega);
        let mut turn = Complex::from_polar(1.0, omega * origin as f32);
        for &sample in raw {
            self.rotated.push(sample * turn);
            turn *= step;
        }
        self.filter.reset();
        self.filter.process(&self.rotated, &mut self.out);
    }
}

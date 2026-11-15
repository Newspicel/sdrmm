use std::f64::consts::FRAC_1_SQRT_2;

use num_complex::Complex;
use sdrmm_dsp::LoopFilter;

use super::{TRACK_BW, codebook::Codebook};

const FREQ_LIMIT_CYCLES_PER_SYMBOL: f64 = 0.25;

pub(super) struct PhaseTracker {
    filter: LoopFilter,
    phase: f64,
}

impl PhaseTracker {
    pub(super) fn new() -> Self {
        Self {
            filter: LoopFilter::new(TRACK_BW, FRAC_1_SQRT_2, FREQ_LIMIT_CYCLES_PER_SYMBOL),
            phase: 0.0,
        }
    }

    pub(super) fn derotate(&self, bank: &mut [Complex<f32>]) {
        let turn = Complex::new((-self.phase).cos() as f32, (-self.phase).sin() as f32);
        for slot in bank.iter_mut() {
            *slot *= turn;
        }
    }

    pub(super) fn follow(
        &mut self,
        bank: &[Complex<f32>],
        label: u32,
        codebook: &Codebook,
        symbol: usize,
    ) {
        let chosen = bank[(label >> 2) as usize % bank.len()];
        let reference = codebook.reference(label, symbol);
        let error = f64::from((chosen * reference.conj()).arg());
        self.phase += self.filter.advance(error);
    }
}

use num_complex::Complex;

use super::{FACTOR, HALF_BANDS, mixer::PeriodicMixer, prototype};
use crate::Decimator;

#[derive(Clone, Debug)]
pub struct SubbandDecimator {
    mixer: Option<PeriodicMixer>,
    filter: Decimator,
    mixed: Vec<Complex<f32>>,
}

impl SubbandDecimator {
    pub(super) fn new(band: usize, block_len: usize) -> Self {
        let mut result = Self {
            mixer: (band != HALF_BANDS).then(|| PeriodicMixer::new(band)),
            filter: Decimator::new(&prototype(), FACTOR),
            mixed: vec![Complex::new(0.0, 0.0); block_len],
        };
        let mut output = Vec::new();
        result.filter.process(&result.mixed, &mut output);
        result.reset();
        result
    }

    pub fn reset(&mut self) {
        if let Some(mixer) = &mut self.mixer {
            mixer.reset();
        }
        self.filter.reset();
    }

    pub fn process(&mut self, input: &[Complex<f32>], output: &mut Vec<Complex<f32>>) {
        if let Some(mixer) = &mut self.mixer {
            self.mixed.resize(input.len(), Complex::new(0.0, 0.0));
            mixer.mix_into(input, &mut self.mixed);
            self.filter.process(&self.mixed, output);
        } else {
            self.filter.process(input, output);
        }
    }
}

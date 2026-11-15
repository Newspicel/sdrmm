use num_complex::Complex;

use super::{CHIPS, params::CckParams};

#[derive(Clone, Debug)]
pub struct CckMod {
    params: CckParams,
    chips: Vec<Complex<f32>>,
}

impl CckMod {
    #[must_use]
    pub fn new(params: CckParams) -> Self {
        Self {
            params,
            chips: Vec::new(),
        }
    }

    #[must_use]
    pub fn params(&self) -> &CckParams {
        &self.params
    }

    pub fn frame(&mut self, preamble: &[u32], labels: &[u32], out: &mut Vec<Complex<f32>>) {
        self.chips.clear();
        let codebook = &self.params.codebook;
        codebook.push_chips(preamble, 0, &mut self.chips);
        codebook.push_chips(labels, preamble.len(), &mut self.chips);
        self.params.shaper.render(&self.chips, out);
    }

    #[must_use]
    pub fn preamble_chips(&self, preamble: &[u32]) -> Vec<Complex<f32>> {
        let mut chips = Vec::with_capacity(preamble.len() * CHIPS);
        self.params.codebook.push_chips(preamble, 0, &mut chips);
        chips
    }
}

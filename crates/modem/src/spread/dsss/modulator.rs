use num_complex::Complex;

use super::params::DsssParams;

#[derive(Clone, Debug)]
pub struct DsssMod {
    params: DsssParams,
    chips: Vec<Complex<f32>>,
}

impl DsssMod {
    #[must_use]
    pub fn new(params: DsssParams) -> Self {
        Self {
            params,
            chips: Vec::new(),
        }
    }

    #[must_use]
    pub fn params(&self) -> &DsssParams {
        &self.params
    }

    pub fn frame(
        &mut self,
        preamble: &[Complex<f32>],
        points: &[Complex<f32>],
        out: &mut Vec<Complex<f32>>,
    ) {
        self.chips.clear();
        self.params.spread_into(preamble, &mut self.chips);
        self.params.spread_into(points, &mut self.chips);
        self.params.shaper.render(&self.chips, out);
    }

    #[must_use]
    pub fn preamble_chips(&self, preamble: &[Complex<f32>]) -> Vec<Complex<f32>> {
        let mut chips = Vec::new();
        self.params.spread_into(preamble, &mut chips);
        chips
    }
}

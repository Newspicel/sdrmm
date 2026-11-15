use num_complex::Complex;

use super::{OtfsGrid, OtfsPrecoder};
use crate::ofdm::{OfdmMod, OfdmParams};

#[derive(Clone)]
pub struct OtfsMod {
    carrier: OfdmMod,
    precoder: OtfsPrecoder,
    tf: Vec<Complex<f32>>,
}

impl OtfsMod {
    #[must_use]
    pub fn new(params: OfdmParams, grid: OtfsGrid) -> Self {
        assert_eq!(
            grid.delay,
            params.data_subcarriers(),
            "the delay axis spans the data subcarriers"
        );
        Self {
            carrier: OfdmMod::new(params),
            precoder: OtfsPrecoder::new(grid),
            tf: vec![Complex::new(0.0, 0.0); grid.points()],
        }
    }

    pub fn frame(&mut self, dd: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        self.precoder.spread(dd, &mut self.tf);
        self.carrier.frame(&self.tf, out);
    }
}

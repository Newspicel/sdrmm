use num_complex::Complex;
use sdrmm_dsp::math::complex::narrow;

use super::{GfdmParams, GfdmPreamble};
use crate::multicarrier::transform::matvec;

#[derive(Clone)]
pub struct GfdmMod {
    params: GfdmParams,
    matrix: Vec<Complex<f32>>,
    block: Vec<Complex<f32>>,
    preamble: Vec<Complex<f32>>,
}

impl GfdmMod {
    #[must_use]
    pub fn new(params: GfdmParams) -> Self {
        assert!(
            params.cp <= params.block(),
            "a cyclic prefix is a copy of the block's tail; {} samples of a {}-sample block is \
             not one",
            params.cp,
            params.block()
        );
        let matrix = params.matrix().into_iter().map(narrow).collect();
        Self {
            matrix,
            block: vec![Complex::new(0.0, 0.0); params.block()],
            preamble: GfdmPreamble::new(&params).block().to_vec(),
            params,
        }
    }

    #[must_use]
    pub fn params(&self) -> &GfdmParams {
        &self.params
    }

    pub fn preamble(&self, out: &mut Vec<Complex<f32>>) {
        let n = self.params.block();
        out.extend_from_slice(&self.preamble[n - self.params.cp..]);
        out.extend_from_slice(&self.preamble);
    }

    pub fn frame(&mut self, points: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        self.preamble(out);
        self.modulate(points, out);
    }

    pub fn modulate(&mut self, points: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        let n = self.params.block();
        for chunk in points.chunks_exact(n) {
            matvec(&self.matrix, n, n, chunk, &mut self.block);
            out.extend_from_slice(&self.block[n - self.params.cp..]);
            out.extend_from_slice(&self.block);
        }
    }
}

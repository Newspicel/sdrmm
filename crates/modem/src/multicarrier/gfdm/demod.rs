use num_complex::Complex;
use sdrmm_dsp::math::complex::narrow;

use super::GfdmParams;
use crate::multicarrier::transform::{invert, matvec};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GfdmDetector {
    ZeroForcing,
    Matched,
}

#[derive(Clone)]
pub struct GfdmDemod {
    params: GfdmParams,
    matrix: Vec<Complex<f32>>,
    amplification: Vec<f32>,
    block: Vec<Complex<f32>>,
}

impl GfdmDemod {
    #[must_use]
    pub fn new(params: GfdmParams, detector: GfdmDetector) -> Self {
        let n = params.block();
        let mut a = params.matrix();
        let receive: Vec<Complex<f64>> = match detector {
            GfdmDetector::ZeroForcing => {
                assert!(
                    invert(&mut a, n).is_some(),
                    "the GFDM prototype at roll-off {} over a {}×{} block has no inverse",
                    params.rolloff,
                    params.subcarriers,
                    params.subsymbols
                );
                a
            }
            GfdmDetector::Matched => (0..n * n).map(|i| a[(i % n) * n + i / n].conj()).collect(),
        };
        let amplification = receive
            .chunks_exact(n)
            .map(|row| row.iter().map(|v| v.norm_sqr()).sum::<f64>() as f32)
            .collect();
        Self {
            matrix: receive.into_iter().map(narrow).collect(),
            amplification,
            block: vec![Complex::new(0.0, 0.0); n],
            params,
        }
    }

    #[must_use]
    pub fn params(&self) -> &GfdmParams {
        &self.params
    }

    #[must_use]
    pub fn amplification(&self) -> &[f32] {
        &self.amplification
    }

    pub fn detect_block(&self, window: &[Complex<f32>], out: &mut [Complex<f32>]) {
        let n = self.params.block();
        matvec(&self.matrix, n, n, window, out);
    }

    pub fn demodulate(&mut self, x: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        let n = self.params.block();
        for chunk in x.chunks_exact(self.params.samples()) {
            matvec(
                &self.matrix,
                n,
                n,
                &chunk[self.params.cp..],
                &mut self.block,
            );
            out.extend_from_slice(&self.block);
        }
    }
}

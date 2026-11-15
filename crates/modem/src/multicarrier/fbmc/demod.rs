use num_complex::Complex;

use super::FbmcParams;

#[derive(Clone)]
pub struct FbmcDemod {
    params: FbmcParams,
    kernels: Vec<Vec<Complex<f32>>>,
}

impl FbmcDemod {
    #[must_use]
    pub fn new(params: FbmcParams) -> Self {
        Self {
            kernels: params.kernels(),
            params,
        }
    }

    #[must_use]
    pub fn params(&self) -> &FbmcParams {
        &self.params
    }

    pub fn demodulate(
        &mut self,
        x: &[Complex<f32>],
        symbols: usize,
        out: &mut Vec<Complex<f32>>,
    ) -> usize {
        let (allocated, stride) = (self.params.allocated, self.params.slot_stride());
        let len = self.params.prototype_len();
        for symbol in 0..symbols {
            if (2 * symbol + 1) * stride + len > x.len() {
                return symbol;
            }
            for index in 0..allocated {
                let kernel = &self.kernels[index];
                let mut halves = [0.0f32; 2];
                for (half, slot_value) in halves.iter_mut().enumerate() {
                    let slot = 2 * symbol + half;
                    let base = slot * stride;
                    let mut acc = Complex::new(0.0f32, 0.0);
                    for (u, &tap) in kernel.iter().enumerate() {
                        acc += x[base + u] * tap.conj();
                    }
                    *slot_value = (acc * self.params.oqam_phase(index, slot).conj()).re;
                }
                out.push(Complex::new(halves[0], halves[1]));
            }
        }
        symbols
    }
}

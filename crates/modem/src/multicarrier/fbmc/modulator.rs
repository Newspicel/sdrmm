use num_complex::Complex;

use super::FbmcParams;

#[derive(Clone)]
pub struct FbmcMod {
    params: FbmcParams,
    kernels: Vec<Vec<Complex<f32>>>,
}

impl FbmcMod {
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

    pub fn modulate(&mut self, points: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        let allocated = self.params.allocated;
        let stride = self.params.slot_stride();
        let start = out.len();
        out.resize(
            start + self.params.frame_samples(points.len()),
            Complex::new(0.0, 0.0),
        );
        for (symbol, chunk) in points.chunks_exact(allocated).enumerate() {
            for (index, &point) in chunk.iter().enumerate() {
                let kernel = &self.kernels[index];
                for (half, value) in [point.re, point.im].into_iter().enumerate() {
                    let slot = 2 * symbol + half;
                    let phase = self.params.oqam_phase(index, slot) * value;
                    let base = start + slot * stride;
                    for (u, &tap) in kernel.iter().enumerate() {
                        out[base + u] += phase * tap;
                    }
                }
            }
        }
    }
}

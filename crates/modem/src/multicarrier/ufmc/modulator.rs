use num_complex::Complex;
use sdrmm_dsp::math::fft::FftPair;

use super::UfmcParams;

#[derive(Clone)]
pub struct UfmcMod {
    params: UfmcParams,
    dft: FftPair,
    filters: Vec<Vec<Complex<f32>>>,
    grid: Vec<Complex<f32>>,
    symbol: Vec<Complex<f32>>,
}

impl UfmcMod {
    #[must_use]
    pub fn new(params: UfmcParams) -> Self {
        Self {
            dft: FftPair::new(params.fft),
            filters: (0..params.subbands)
                .map(|b| params.subband_filter(b))
                .collect(),
            grid: vec![Complex::new(0.0, 0.0); params.fft],
            symbol: vec![Complex::new(0.0, 0.0); params.samples()],
            params,
        }
    }

    #[must_use]
    pub fn params(&self) -> &UfmcParams {
        &self.params
    }

    pub fn modulate(&mut self, points: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        let per_symbol = self.params.points();
        for chunk in points.chunks_exact(per_symbol) {
            self.symbol.fill(Complex::new(0.0, 0.0));
            for b in 0..self.params.subbands {
                self.grid.fill(Complex::new(0.0, 0.0));
                let first = b * self.params.per_subband;
                for k in 0..self.params.per_subband {
                    self.grid[self.params.bin(first + k)] = chunk[first + k];
                }
                self.dft.inverse_unitary(&mut self.grid);
                for (n, &x) in self.grid.iter().enumerate() {
                    for (l, &tap) in self.filters[b].iter().enumerate() {
                        self.symbol[n + l] += x * tap;
                    }
                }
            }
            out.extend_from_slice(&self.symbol);
        }
    }
}

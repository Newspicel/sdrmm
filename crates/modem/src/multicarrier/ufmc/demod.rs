use num_complex::Complex;
use sdrmm_dsp::math::{
    complex::{narrow, widen},
    fft::FftPair,
};

use super::UfmcParams;

#[derive(Clone)]
pub struct UfmcDemod {
    params: UfmcParams,
    dft: FftPair,
    equaliser: Vec<Complex<f32>>,
    amplification: Vec<f32>,
    padded: Vec<Complex<f32>>,
}

impl UfmcDemod {
    #[must_use]
    pub fn new(params: UfmcParams) -> Self {
        let filters: Vec<Vec<Complex<f32>>> = (0..params.subbands)
            .map(|b| params.subband_filter(b))
            .collect();
        let equaliser: Vec<Complex<f32>> = (0..params.points())
            .map(|index| {
                let bin = params.bin(index) as f64;
                let response: Complex<f64> = filters[params.subband_of(index)]
                    .iter()
                    .enumerate()
                    .map(|(n, &tap)| {
                        widen(tap)
                            * Complex::from_polar(
                                1.0,
                                -std::f64::consts::TAU * bin * n as f64 / params.fft as f64,
                            )
                    })
                    .sum();
                narrow(response.inv())
            })
            .collect();
        Self {
            dft: FftPair::new(2 * params.fft),
            amplification: equaliser.iter().map(Complex::norm_sqr).collect(),
            equaliser,
            padded: vec![Complex::new(0.0, 0.0); 2 * params.fft],
            params,
        }
    }

    #[must_use]
    pub fn params(&self) -> &UfmcParams {
        &self.params
    }

    #[must_use]
    pub fn amplification(&self) -> &[f32] {
        &self.amplification
    }

    pub fn demodulate(&mut self, x: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        let samples = self.params.samples();
        for chunk in x.chunks_exact(samples) {
            self.padded.fill(Complex::new(0.0, 0.0));
            self.padded[..samples].copy_from_slice(chunk);
            self.dft.forward_unitary(&mut self.padded);
            for (index, &gain) in self.equaliser.iter().enumerate() {
                let bin = 2 * self.params.bin(index);
                out.push(self.padded[bin] * gain * std::f32::consts::SQRT_2);
            }
        }
    }
}

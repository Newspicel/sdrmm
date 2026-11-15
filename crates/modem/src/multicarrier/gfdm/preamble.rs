use num_complex::Complex;
use sdrmm_dsp::math::fft::FftPair;

use super::GfdmParams;

/// Seed of the fixed xorshift sequence the preamble's QPSK spectrum is drawn from.
const SEED: u32 = 0x6fd3_a5c1;
const XORSHIFT_SHIFTS: [u32; 3] = [13, 17, 5];
const MIN_BLOCK: usize = 4;

#[derive(Clone, Debug)]
pub struct GfdmPreamble {
    spectrum: Vec<Complex<f32>>,
    block: Vec<Complex<f32>>,
}

impl GfdmPreamble {
    #[must_use]
    pub fn new(params: &GfdmParams) -> Self {
        let n = params.block();
        assert!(
            n >= MIN_BLOCK && n.is_multiple_of(2),
            "a two-half preamble needs an even block, got {n}"
        );
        let spectrum = qpsk_sequence(n / 2);
        let mut half = spectrum.clone();
        FftPair::new(n / 2).inverse_unitary(&mut half);
        let block = half.iter().chain(&half).copied().collect();
        Self { spectrum, block }
    }

    #[must_use]
    pub fn half(&self) -> usize {
        self.spectrum.len()
    }

    #[must_use]
    pub fn spectrum(&self) -> &[Complex<f32>] {
        &self.spectrum
    }

    #[must_use]
    pub fn block(&self) -> &[Complex<f32>] {
        &self.block
    }
}

fn qpsk_sequence(len: usize) -> Vec<Complex<f32>> {
    let mut state = SEED;
    let [a, b, c] = XORSHIFT_SHIFTS;
    (0..len)
        .map(|_| {
            state ^= state << a;
            state ^= state >> b;
            state ^= state << c;
            let i = if state & 1 == 0 { 1.0 } else { -1.0 };
            let q = if state & 2 == 0 { 1.0 } else { -1.0 };
            Complex::new(i, q) * std::f32::consts::FRAC_1_SQRT_2
        })
        .collect()
}

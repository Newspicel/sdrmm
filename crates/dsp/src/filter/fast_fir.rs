use num_complex::Complex;

use crate::math::{complex::mul_in_place, fft::FftPair};

/// Overlap-save FFT convolver for long FIR filters, with the causal streaming output of a
/// time-domain FIR and no allocation in `process`.
///
/// Every call ends on a partial block that costs one more transform pair, so it pays off on
/// blocks near [`Self::fft_len`] or longer and loses to a direct FIR on short ones.
#[derive(Clone)]
pub struct FastFir {
    fft: FftPair<f32>,
    fft_len: usize,
    overlap: usize,
    step: usize,
    spectrum: Vec<Complex<f32>>,
    buf: Vec<Complex<f32>>,
    work: Vec<Complex<f32>>,
    fill: usize,
}

impl FastFir {
    /// Builds an overlap-save convolver for real taps.
    #[must_use]
    pub fn real(taps: &[f32]) -> Self {
        let complex: Vec<Complex<f32>> = taps.iter().map(|&t| Complex::new(t, 0.0)).collect();
        Self::complex(&complex)
    }

    /// Builds an overlap-save convolver for complex taps.
    ///
    /// # Panics
    /// Panics if `taps` is empty.
    #[must_use]
    pub fn complex(taps: &[Complex<f32>]) -> Self {
        assert!(!taps.is_empty(), "taps must not be empty");
        let overlap = taps.len() - 1;
        let fft_len = (4 * taps.len()).next_power_of_two().max(64);
        let step = fft_len - overlap;
        let mut fft = FftPair::new(fft_len);
        let mut spectrum = vec![Complex::new(0.0, 0.0); fft_len];
        let inv = 1.0 / fft_len as f32;
        for (dst, &tap) in spectrum.iter_mut().zip(taps) {
            *dst = tap * inv;
        }
        fft.forward(&mut spectrum);
        Self {
            fft,
            fft_len,
            overlap,
            step,
            spectrum,
            buf: vec![Complex::new(0.0, 0.0); fft_len],
            work: vec![Complex::new(0.0, 0.0); fft_len],
            fill: overlap,
        }
    }

    #[must_use]
    pub const fn fft_len(&self) -> usize {
        self.fft_len
    }

    pub fn reset(&mut self) {
        self.buf.fill(Complex::new(0.0, 0.0));
        self.fill = self.overlap;
    }

    pub fn process(&mut self, input: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        out.clear();
        if input.is_empty() {
            return;
        }
        out.reserve(input.len());
        let mut pos = 0;
        let n = input.len();
        while pos < n {
            let take = (self.fft_len - self.fill).min(n - pos);
            self.buf[self.fill..self.fill + take].copy_from_slice(&input[pos..pos + take]);
            self.fill += take;
            pos += take;
            if self.fill == self.fft_len {
                self.convolve_block(self.step, out);
                self.buf.copy_within(self.step.., 0);
                self.fill = self.overlap;
            }
        }
        let pending = self.fill - self.overlap;
        if pending > 0 {
            self.work[..self.fill].copy_from_slice(&self.buf[..self.fill]);
            self.work[self.fill..].fill(Complex::new(0.0, 0.0));
            self.fft.forward(&mut self.work);
            mul_in_place(&mut self.work, &self.spectrum);
            self.fft.inverse(&mut self.work);
            out.extend_from_slice(&self.work[self.overlap..self.overlap + pending]);
            self.buf.copy_within(pending..self.fill, 0);
            self.fill = self.overlap;
        }
    }

    fn convolve_block(&mut self, emit: usize, out: &mut Vec<Complex<f32>>) {
        self.work.copy_from_slice(&self.buf);
        self.fft.forward(&mut self.work);
        mul_in_place(&mut self.work, &self.spectrum);
        self.fft.inverse(&mut self.work);
        out.extend_from_slice(&self.work[self.overlap..self.overlap + emit]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Decimator, FirC,
        filter::fir::{design_lowpass_kaiser, in_ragged_blocks},
        testutil::complex_tone,
    };

    #[test]
    fn fast_fir_matches_time_domain_fir_across_ragged_blocks() {
        let taps = design_lowpass_kaiser(255, 0.15, 90.0);
        let input: Vec<Complex<f32>> = (0..12_345)
            .map(|k| {
                Complex::new(
                    ((k * 37) % 251) as f32 / 125.0 - 1.0,
                    ((k * 71) % 257) as f32 / 128.0 - 1.0,
                )
            })
            .collect();
        let mut ref_fir = Decimator::new(&taps, 1);
        let mut expected = Vec::new();
        ref_fir.process(&input, &mut expected);

        let mut fast = FastFir::real(&taps);
        let got = in_ragged_blocks(&input, &[1, 17, 255, 1024, 3333, 500], |chunk, out| {
            fast.process(chunk, out);
        });
        assert_eq!(got.len(), expected.len());
        for (i, (a, b)) in got.iter().zip(&expected).enumerate() {
            assert!((a - b).norm() < 5e-5, "sample {i}: {a} vs {b}");
        }

        fast.reset();
        let mut whole = Vec::new();
        fast.process(&input, &mut whole);
        for (i, (a, b)) in whole.iter().zip(&expected).enumerate() {
            assert!((a - b).norm() < 5e-5, "reset sample {i}: {a} vs {b}");
        }
    }

    #[test]
    fn complex_taps_match_firc() {
        let real = design_lowpass_kaiser(129, 0.12, 80.0);
        let taps: Vec<Complex<f32>> = real
            .iter()
            .enumerate()
            .map(|(k, &v)| Complex::from_polar(v, 0.3 * k as f32))
            .collect();
        let input = complex_tone(0.05, 4_096);
        let mut ref_fir = FirC::new(&taps);
        let mut expected = Vec::new();
        ref_fir.process(&input, &mut expected);

        let mut fast = FastFir::complex(&taps);
        let mut got = Vec::new();
        fast.process(&input, &mut got);
        assert_eq!(got.len(), expected.len());
        for (i, (a, b)) in got.iter().zip(&expected).enumerate() {
            assert!((a - b).norm() < 5e-5, "sample {i}: {a} vs {b}");
        }
    }
}

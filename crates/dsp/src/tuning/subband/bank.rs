use num_complex::Complex;

use super::{
    FACTOR, HALF_BANDS, MIXER_PERIOD, MIXER_STEP, SUBBANDS, prototype, transform::Inverse25,
};
use crate::filter::fir::Accumulate;

const ROTATION_PHASES: usize = MIXER_PERIOD / FACTOR;
const BAND_BINS: [usize; SUBBANDS] = [
    ((MIXER_PERIOD - HALF_BANDS) * MIXER_STEP) % MIXER_PERIOD,
    ((1 + MIXER_PERIOD - HALF_BANDS) * MIXER_STEP) % MIXER_PERIOD,
    ((2 + MIXER_PERIOD - HALF_BANDS) * MIXER_STEP) % MIXER_PERIOD,
    ((3 + MIXER_PERIOD - HALF_BANDS) * MIXER_STEP) % MIXER_PERIOD,
    ((4 + MIXER_PERIOD - HALF_BANDS) * MIXER_STEP) % MIXER_PERIOD,
    ((5 + MIXER_PERIOD - HALF_BANDS) * MIXER_STEP) % MIXER_PERIOD,
    ((6 + MIXER_PERIOD - HALF_BANDS) * MIXER_STEP) % MIXER_PERIOD,
    ((7 + MIXER_PERIOD - HALF_BANDS) * MIXER_STEP) % MIXER_PERIOD,
    ((8 + MIXER_PERIOD - HALF_BANDS) * MIXER_STEP) % MIXER_PERIOD,
    ((9 + MIXER_PERIOD - HALF_BANDS) * MIXER_STEP) % MIXER_PERIOD,
    ((10 + MIXER_PERIOD - HALF_BANDS) * MIXER_STEP) % MIXER_PERIOD,
    ((11 + MIXER_PERIOD - HALF_BANDS) * MIXER_STEP) % MIXER_PERIOD,
    ((12 + MIXER_PERIOD - HALF_BANDS) * MIXER_STEP) % MIXER_PERIOD,
];

pub struct SubbandFilterBank {
    tap_len: usize,
    padded_len: usize,
    rev_taps: Vec<[f32; MIXER_PERIOD]>,
    input: Vec<Complex<f32>>,
    start: usize,
    fft: Inverse25,
    output: [Vec<Complex<f32>>; SUBBANDS],
    phase: usize,
}

impl SubbandFilterBank {
    pub(super) fn new(block_len: usize) -> Self {
        let taps = prototype();
        let tap_len = taps.len();
        let blocks = tap_len.div_ceil(MIXER_PERIOD);
        let padded_len = blocks * MIXER_PERIOD;
        let pad = padded_len - tap_len;
        let mut rev_taps = vec![[0.0f32; MIXER_PERIOD]; blocks];
        for (idx, &tap) in taps.iter().enumerate() {
            let chunk = idx / MIXER_PERIOD;
            let k = idx % MIXER_PERIOD;
            let pos = padded_len - 1 - chunk * MIXER_PERIOD - k;
            rev_taps[pos / MIXER_PERIOD][pos % MIXER_PERIOD] = tap;
        }
        let history = tap_len - 1 + pad;
        let mut input = Vec::with_capacity(history + block_len * 2);
        input.resize(history, Complex::new(0.0, 0.0));
        Self {
            tap_len,
            padded_len,
            rev_taps,
            input,
            start: 0,
            fft: Inverse25::new(),
            output: std::array::from_fn(|_| Vec::with_capacity(block_len.div_ceil(FACTOR))),
            phase: 0,
        }
    }

    pub fn reset(&mut self) {
        self.input.clear();
        self.input
            .resize(self.padded_len - 1, Complex::new(0.0, 0.0));
        self.start = 0;
        for output in &mut self.output {
            output.clear();
        }
        self.phase = 0;
    }

    pub fn process(&mut self, input: &[Complex<f32>]) {
        if self.start > 0 && self.input.len() + input.len() > self.input.capacity() {
            self.input.copy_within(self.start.., 0);
            self.input.truncate(self.input.len() - self.start);
            self.start = 0;
        }
        self.input.extend_from_slice(input);
        let available = self.input.len() - self.start;
        let samples = available.saturating_sub(self.padded_len - 1);
        let frames = samples.div_ceil(FACTOR);
        for output in &mut self.output {
            output.clear();
            output.reserve(frames);
        }
        let mut frame = 0;
        #[cfg(target_arch = "aarch64")]
        {
            while frame + 4 <= frames {
                self.process_four(frame);
                frame += 4;
            }
        }
        while frame < frames {
            self.process_one(frame);
            frame += 1;
        }
        self.phase = (self.phase + frames) % ROTATION_PHASES;
        self.start += frames * FACTOR;
        if self.start >= 4096 {
            self.input.copy_within(self.start.., 0);
            self.input.truncate(self.input.len() - self.start);
            self.start = 0;
        }
        let _ = self.tap_len;
    }

    fn process_one(&mut self, frame: usize) {
        let base = self.start + frame * FACTOR;
        let mut raw = [Complex::new(0.0, 0.0); MIXER_PERIOD];
        for (block_idx, taps) in self.rev_taps.iter().enumerate() {
            let chunk = &self.input[base + block_idx * MIXER_PERIOD..];
            for ((sum, &sample), &tap) in raw.iter_mut().zip(chunk).zip(taps) {
                *sum = sum.add_product(sample, tap);
            }
        }
        let rot = ((self.phase + frame) % ROTATION_PHASES) * FACTOR;
        let mut transform = [Complex::new(0.0, 0.0); MIXER_PERIOD];
        for (dst, out) in transform.iter_mut().enumerate() {
            let k = (dst + rot) % MIXER_PERIOD;
            *out = raw[MIXER_PERIOD - 1 - k];
        }
        self.fft.process(&mut transform);
        for (band, output) in self.output.iter_mut().enumerate() {
            output.push(transform[BAND_BINS[band]]);
        }
    }

    #[cfg(target_arch = "aarch64")]
    fn process_four(&mut self, frame: usize) {
        use std::arch::aarch64::{
            float32x4_t, float32x4x2_t, vdupq_n_f32, vfmaq_f32, vld1q_f32, vld2q_f32, vst1q_f32,
            vst2q_f32,
        };
        let mut sums_re = [[0.0f32; MIXER_PERIOD]; 4];
        let mut sums_im = [[0.0f32; MIXER_PERIOD]; 4];
        unsafe {
            for f in 0..4 {
                let base = self.start + (frame + f) * FACTOR;
                let mut re = [vdupq_n_f32(0.0); 6];
                let mut im = [vdupq_n_f32(0.0); 6];
                let mut last_re = 0.0f32;
                let mut last_im = 0.0f32;
                for (block_idx, taps) in self.rev_taps.iter().enumerate() {
                    let sp = self
                        .input
                        .as_ptr()
                        .add(base + block_idx * MIXER_PERIOD)
                        .cast::<f32>();
                    let tp = taps.as_ptr();
                    for g in 0..6 {
                        let vs = vld2q_f32(sp.add(g * 8));
                        let vt = vld1q_f32(tp.add(g * 4));
                        re[g] = vfmaq_f32(re[g], vs.0, vt);
                        im[g] = vfmaq_f32(im[g], vs.1, vt);
                    }
                    let last = *self
                        .input
                        .get_unchecked(base + block_idx * MIXER_PERIOD + 24);
                    let t24 = taps[24];
                    last_re = last.re.mul_add(t24, last_re);
                    last_im = last.im.mul_add(t24, last_im);
                }
                let rp = sums_re[f].as_mut_ptr();
                let ip = sums_im[f].as_mut_ptr();
                for g in 0..6 {
                    vst1q_f32(rp.add(g * 4), re[g]);
                    vst1q_f32(ip.add(g * 4), im[g]);
                }
                sums_re[f][24] = last_re;
                sums_im[f][24] = last_im;
            }
            let rots: [usize; 4] =
                std::array::from_fn(|f| ((self.phase + frame + f) % ROTATION_PHASES) * FACTOR);
            let mut fft_re: [float32x4_t; MIXER_PERIOD] = [vdupq_n_f32(0.0); MIXER_PERIOD];
            let mut fft_im: [float32x4_t; MIXER_PERIOD] = [vdupq_n_f32(0.0); MIXER_PERIOD];
            for dst in 0..MIXER_PERIOD {
                let r_arr = [
                    sums_re[0][MIXER_PERIOD - 1 - (dst + rots[0]) % MIXER_PERIOD],
                    sums_re[1][MIXER_PERIOD - 1 - (dst + rots[1]) % MIXER_PERIOD],
                    sums_re[2][MIXER_PERIOD - 1 - (dst + rots[2]) % MIXER_PERIOD],
                    sums_re[3][MIXER_PERIOD - 1 - (dst + rots[3]) % MIXER_PERIOD],
                ];
                let i_arr = [
                    sums_im[0][MIXER_PERIOD - 1 - (dst + rots[0]) % MIXER_PERIOD],
                    sums_im[1][MIXER_PERIOD - 1 - (dst + rots[1]) % MIXER_PERIOD],
                    sums_im[2][MIXER_PERIOD - 1 - (dst + rots[2]) % MIXER_PERIOD],
                    sums_im[3][MIXER_PERIOD - 1 - (dst + rots[3]) % MIXER_PERIOD],
                ];
                fft_re[dst] = vld1q_f32(r_arr.as_ptr());
                fft_im[dst] = vld1q_f32(i_arr.as_ptr());
            }
            self.fft.process_4(&mut fft_re, &mut fft_im);
            for (band, output) in self.output.iter_mut().enumerate() {
                let bin = BAND_BINS[band];
                let base = output.len();
                let dst = output.spare_capacity_mut().as_mut_ptr().cast::<f32>();
                vst2q_f32(dst, float32x4x2_t(fft_re[bin], fft_im[bin]));
                output.set_len(base + 4);
            }
        }
    }

    #[must_use]
    pub fn samples(&self, band: usize) -> &[Complex<f32>] {
        &self.output[band]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{math::rng::Rng, tuning::subband::SubbandPlan};

    #[test]
    fn all_bands_match_independent_filters_across_ragged_blocks_and_reset() {
        let plan = SubbandPlan::new(20_000_000.0).unwrap();
        let mut rng = Rng::mixed(0x0123_4567_89ab_cdef);
        let mut uniform = || (2.0 * rng.uniform() - 1.0) as f32;
        let input: Vec<_> = (0..65_537)
            .map(|_| Complex::new(uniform(), uniform()))
            .collect();
        let mut bank = plan.filter_bank(4099);
        let mut filters: Vec<_> = (0..SUBBANDS)
            .map(|band| plan.decimator(band, 4099))
            .collect();
        let mut expected = Vec::new();
        for _ in 0..2 {
            let mut at = 0;
            for size in [0, 1, 17, 5, 25, 2048, 4099].into_iter().cycle() {
                let end = (at + size).min(input.len());
                bank.process(&input[at..end]);
                for (band, filter) in filters.iter_mut().enumerate() {
                    filter.process(&input[at..end], &mut expected);
                    let actual = bank.samples(band);
                    assert_eq!(actual.len(), expected.len());
                    for (actual, expected) in actual.iter().zip(&expected) {
                        assert!(
                            (*actual - *expected).norm() < 2e-6,
                            "band={band} at={at} actual={actual} expected={expected}"
                        );
                    }
                }
                at = end;
                if at == input.len() {
                    break;
                }
            }
            bank.reset();
            for filter in &mut filters {
                filter.reset();
            }
        }
    }
}

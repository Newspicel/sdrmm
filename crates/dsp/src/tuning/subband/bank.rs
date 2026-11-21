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

const ROT_SRC: [[usize; MIXER_PERIOD]; ROTATION_PHASES] = {
    let mut table = [[0usize; MIXER_PERIOD]; ROTATION_PHASES];
    let mut phase = 0;
    while phase < ROTATION_PHASES {
        let rot = phase * FACTOR;
        let mut dst = 0;
        while dst < MIXER_PERIOD {
            table[phase][dst] = MIXER_PERIOD - 1 - (dst + rot) % MIXER_PERIOD;
            dst += 1;
        }
        phase += 1;
    }
    table
};

pub struct SubbandFilterBank {
    tap_len: usize,
    padded_len: usize,
    rev_taps: Vec<[f32; MIXER_PERIOD]>,
    #[cfg(target_arch = "x86_64")]
    rev_taps_dup: Vec<[[f32; 8]; 6]>,
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
        #[cfg(target_arch = "x86_64")]
        let rev_taps_dup: Vec<[[f32; 8]; 6]> = rev_taps
            .iter()
            .map(|taps| {
                std::array::from_fn(|g| {
                    let base = g * 4;
                    [
                        taps[base],
                        taps[base],
                        taps[base + 1],
                        taps[base + 1],
                        taps[base + 2],
                        taps[base + 2],
                        taps[base + 3],
                        taps[base + 3],
                    ]
                })
            })
            .collect();
        let history = tap_len - 1 + pad;
        let mut input = Vec::with_capacity(history + block_len * 2);
        input.resize(history, Complex::new(0.0, 0.0));
        Self {
            tap_len,
            padded_len,
            rev_taps,
            #[cfg(target_arch = "x86_64")]
            rev_taps_dup,
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
        #[cfg(target_arch = "x86_64")]
        {
            if std::arch::is_x86_feature_detected!("avx2")
                && std::arch::is_x86_feature_detected!("fma")
            {
                while frame + 8 <= frames {
                    unsafe { self.process_eight(frame) };
                    frame += 8;
                }
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
            let phases: [usize; 4] =
                std::array::from_fn(|f| (self.phase + frame + f) % ROTATION_PHASES);
            let mut fft_re: [float32x4_t; MIXER_PERIOD] = [vdupq_n_f32(0.0); MIXER_PERIOD];
            let mut fft_im: [float32x4_t; MIXER_PERIOD] = [vdupq_n_f32(0.0); MIXER_PERIOD];
            for dst in 0..MIXER_PERIOD {
                let i0 = ROT_SRC[phases[0]][dst];
                let i1 = ROT_SRC[phases[1]][dst];
                let i2 = ROT_SRC[phases[2]][dst];
                let i3 = ROT_SRC[phases[3]][dst];
                let r_arr = [
                    sums_re[0][i0],
                    sums_re[1][i1],
                    sums_re[2][i2],
                    sums_re[3][i3],
                ];
                let i_arr = [
                    sums_im[0][i0],
                    sums_im[1][i1],
                    sums_im[2][i2],
                    sums_im[3][i3],
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

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2", enable = "fma")]
    unsafe fn process_eight(&mut self, frame: usize) {
        use std::arch::x86_64::{
            __m256, _mm256_castpd_ps, _mm256_castps_pd, _mm256_fmadd_ps, _mm256_loadu_ps,
            _mm256_permute2f128_ps, _mm256_permute4x64_pd, _mm256_setzero_ps, _mm256_shuffle_ps,
            _mm256_storeu_ps, _mm256_unpackhi_ps, _mm256_unpacklo_ps,
        };
        let mut sums_re = [[0.0f32; MIXER_PERIOD]; 8];
        let mut sums_im = [[0.0f32; MIXER_PERIOD]; 8];
        for f in 0..8 {
            let base = self.start + (frame + f) * FACTOR;
            let mut acc = [_mm256_setzero_ps(); 6];
            let mut last_re = 0.0f32;
            let mut last_im = 0.0f32;
            for (block_idx, (taps, dup)) in self.rev_taps.iter().zip(&self.rev_taps_dup).enumerate()
            {
                let sp = unsafe {
                    self.input
                        .as_ptr()
                        .add(base + block_idx * MIXER_PERIOD)
                        .cast::<f32>()
                };
                for g in 0..6 {
                    let vs = unsafe { _mm256_loadu_ps(sp.add(g * 8)) };
                    let vt = unsafe { _mm256_loadu_ps(dup[g].as_ptr()) };
                    acc[g] = _mm256_fmadd_ps(vs, vt, acc[g]);
                }
                let last = unsafe {
                    *self
                        .input
                        .get_unchecked(base + block_idx * MIXER_PERIOD + 24)
                };
                let t24 = taps[24];
                last_re = last.re.mul_add(t24, last_re);
                last_im = last.im.mul_add(t24, last_im);
            }
            let rp = sums_re[f].as_mut_ptr();
            let ip = sums_im[f].as_mut_ptr();
            for pair in 0..3 {
                let lo = acc[pair * 2];
                let hi = acc[pair * 2 + 1];
                let re_shuf = _mm256_shuffle_ps::<0x88>(lo, hi);
                let im_shuf = _mm256_shuffle_ps::<0xDD>(lo, hi);
                let re_ord =
                    _mm256_castpd_ps(_mm256_permute4x64_pd::<0xD8>(_mm256_castps_pd(re_shuf)));
                let im_ord =
                    _mm256_castpd_ps(_mm256_permute4x64_pd::<0xD8>(_mm256_castps_pd(im_shuf)));
                unsafe {
                    _mm256_storeu_ps(rp.add(pair * 8), re_ord);
                    _mm256_storeu_ps(ip.add(pair * 8), im_ord);
                }
            }
            sums_re[f][24] = last_re;
            sums_im[f][24] = last_im;
        }
        let phases: [usize; 8] =
            std::array::from_fn(|f| (self.phase + frame + f) % ROTATION_PHASES);
        let mut fft_re: [__m256; MIXER_PERIOD] = [_mm256_setzero_ps(); MIXER_PERIOD];
        let mut fft_im: [__m256; MIXER_PERIOD] = [_mm256_setzero_ps(); MIXER_PERIOD];
        for dst in 0..MIXER_PERIOD {
            let idx: [usize; 8] = std::array::from_fn(|f| ROT_SRC[phases[f]][dst]);
            let r_arr: [f32; 8] = std::array::from_fn(|f| sums_re[f][idx[f]]);
            let i_arr: [f32; 8] = std::array::from_fn(|f| sums_im[f][idx[f]]);
            fft_re[dst] = unsafe { _mm256_loadu_ps(r_arr.as_ptr()) };
            fft_im[dst] = unsafe { _mm256_loadu_ps(i_arr.as_ptr()) };
        }
        unsafe { self.fft.process_8(&mut fft_re, &mut fft_im) };
        for (band, output) in self.output.iter_mut().enumerate() {
            let bin = BAND_BINS[band];
            let base = output.len();
            let dst = output.spare_capacity_mut().as_mut_ptr().cast::<f32>();
            let lo = _mm256_unpacklo_ps(fft_re[bin], fft_im[bin]);
            let hi = _mm256_unpackhi_ps(fft_re[bin], fft_im[bin]);
            unsafe {
                _mm256_storeu_ps(dst, _mm256_permute2f128_ps::<0x20>(lo, hi));
                _mm256_storeu_ps(dst.add(8), _mm256_permute2f128_ps::<0x31>(lo, hi));
                output.set_len(base + 8);
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

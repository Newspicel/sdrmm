use num_complex::Complex;

use crate::{
    math::complex::{magnitudes_into, mul_conj_in_place, sum_norm_sqr},
    math::db::Decibels,
    math::fft::FftPair,
    math::special::parabolic_peak,
};

/// What one lane looks like measured against another: how far behind it runs, by how much its
/// amplitude and phase differ, and how much of the two is genuinely the same signal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DelayEstimate {
    /// Positive when `b` lags `a`.
    pub delay_samples: f32,
    pub phase_rad: f32,
    pub gain: f32,
    pub coherence: f32,
    pub peak_to_floor_db: f32,
}

impl DelayEstimate {
    #[must_use]
    pub const fn nothing() -> Self {
        Self {
            delay_samples: 0.0,
            phase_rad: 0.0,
            gain: 0.0,
            coherence: 0.0,
            peak_to_floor_db: 0.0,
        }
    }
}

#[must_use]
pub fn straddled_coherence(peak: f32, side: f32, energies: f32) -> f32 {
    if energies > f32::MIN_POSITIVE {
        ((peak * peak + side * side) / energies).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn peak_to_floor_db(peak: f32, total: f32, lags: usize) -> f32 {
    let floor = (total - peak) / lags.saturating_sub(1).max(1) as f32;
    if floor <= f32::MIN_POSITIVE {
        return f32::INFINITY;
    }
    (peak / floor).power_to_db()
}

/// FFT cross-correlation sized once for a frame length, then reused.
///
/// The transform runs at twice the frame length so the correlation is linear rather than
/// circular: a lag near the frame length must not wrap round and be read as a lag near zero.
pub struct XCorr {
    fft: FftPair,
    frame: usize,
    a: Vec<Complex<f32>>,
    b: Vec<Complex<f32>>,
}

impl XCorr {
    #[must_use]
    pub fn new(frame: usize) -> Self {
        let size = (frame * 2).next_power_of_two().max(2);
        Self {
            fft: FftPair::new(size),
            frame,
            a: vec![Complex::default(); size],
            b: vec![Complex::default(); size],
        }
    }

    #[must_use]
    pub const fn frame(&self) -> usize {
        self.frame
    }

    /// Fills `out` with `r[τ] = Σ b[n + τ] · conj(a[n])` for lags `-frame..frame`, in that order,
    /// so index `frame` is lag zero.
    pub fn correlate(&mut self, a: &[Complex<f32>], b: &[Complex<f32>], out: &mut Vec<f32>) {
        let frame = self.frame;
        let cross = self.cross(a, b);
        let size = cross.len();
        out.clear();
        out.reserve(2 * frame);
        magnitudes_into(&cross[size - frame..size], out);
        magnitudes_into(&cross[..frame], out);
    }

    /// The whole measurement in one pass: peak lag to a fraction of a sample, and the complex
    /// ratio and coherence at that lag.
    pub fn estimate(&mut self, a: &[Complex<f32>], b: &[Complex<f32>]) -> DelayEstimate {
        let frame = self.frame;
        let energy_a = sum_norm_sqr(&a[..a.len().min(frame)]);
        let energy_b = sum_norm_sqr(&b[..b.len().min(frame)]);
        if energy_a <= f32::MIN_POSITIVE || energy_b <= f32::MIN_POSITIVE {
            return DelayEstimate::nothing();
        }
        let cross = self.cross_unscaled(a, b);
        let size = cross.len();
        let inv = 1.0 / size as f32;
        let (peak, best, total) = scan_lags(cross, frame);
        let at = |lag: isize| cross[lag.rem_euclid(size as isize) as usize];
        let value = at(peak);
        let (left, right) = (at(peak - 1).norm() * inv, at(peak + 1).norm() * inv);
        let centre = value.norm() * inv;
        let fraction = parabolic_peak(f64::from(left), f64::from(centre), f64::from(right)) as f32;
        DelayEstimate {
            delay_samples: peak as f32 + fraction,
            phase_rad: value.arg(),
            gain: centre / energy_a,
            coherence: straddled_coherence(centre, left.max(right), energy_a * energy_b),
            peak_to_floor_db: peak_to_floor_db(best, total, 2 * frame),
        }
    }

    fn cross_unscaled(&mut self, a: &[Complex<f32>], b: &[Complex<f32>]) -> &[Complex<f32>] {
        self.cross_spectrum(a, b);
        self.fft.inverse(&mut self.b);
        &self.b
    }

    fn cross(&mut self, a: &[Complex<f32>], b: &[Complex<f32>]) -> &[Complex<f32>] {
        self.cross_spectrum(a, b);
        self.fft.inverse_scaled(&mut self.b);
        &self.b
    }

    fn cross_spectrum(&mut self, a: &[Complex<f32>], b: &[Complex<f32>]) {
        let size = self.fft.len();
        let frame = self.frame;
        let na = a.len().min(frame);
        self.a[..na].copy_from_slice(&a[..na]);
        self.a[na..size].fill(Complex::default());
        let nb = b.len().min(frame);
        self.b[..nb].copy_from_slice(&b[..nb]);
        self.b[nb..size].fill(Complex::default());
        self.fft.forward(&mut self.a);
        self.fft.forward(&mut self.b);
        mul_conj_in_place(&mut self.b, &self.a);
    }
}

fn scan_slice(slice: &[Complex<f32>], lag_base: isize, peak: &mut isize, best: &mut f32) -> f32 {
    #[cfg(target_arch = "aarch64")]
    {
        use std::arch::aarch64::{
            vaddq_f32, vaddvq_f32, vdupq_n_f32, vfmaq_f32, vld2q_f32, vmaxvq_f32, vmulq_f32,
            vst1q_f32,
        };
        let (chunks, tail) = slice.as_chunks::<4>();
        let mut total_v0 = unsafe { vdupq_n_f32(0.0) };
        let mut total_v1 = unsafe { vdupq_n_f32(0.0) };
        let mut cur_best = *best;
        let mut cur_peak = *peak;
        unsafe {
            let mut idx = 0usize;
            while idx + 1 < chunks.len() {
                let v0 = vld2q_f32(chunks.get_unchecked(idx).as_ptr().cast());
                let v1 = vld2q_f32(chunks.get_unchecked(idx + 1).as_ptr().cast());
                let p0 = vfmaq_f32(vmulq_f32(v0.0, v0.0), v0.1, v0.1);
                let p1 = vfmaq_f32(vmulq_f32(v1.0, v1.0), v1.1, v1.1);
                total_v0 = vaddq_f32(total_v0, p0);
                total_v1 = vaddq_f32(total_v1, p1);
                if vmaxvq_f32(p0) > cur_best {
                    let mut arr = [0.0f32; 4];
                    vst1q_f32(arr.as_mut_ptr(), p0);
                    for (lane, &power) in arr.iter().enumerate() {
                        if power > cur_best {
                            cur_best = power;
                            cur_peak = lag_base + (idx * 4 + lane) as isize;
                        }
                    }
                }
                if vmaxvq_f32(p1) > cur_best {
                    let mut arr = [0.0f32; 4];
                    vst1q_f32(arr.as_mut_ptr(), p1);
                    for (lane, &power) in arr.iter().enumerate() {
                        if power > cur_best {
                            cur_best = power;
                            cur_peak = lag_base + ((idx + 1) * 4 + lane) as isize;
                        }
                    }
                }
                idx += 2;
            }
            if idx < chunks.len() {
                let v0 = vld2q_f32(chunks.get_unchecked(idx).as_ptr().cast());
                let p0 = vfmaq_f32(vmulq_f32(v0.0, v0.0), v0.1, v0.1);
                total_v0 = vaddq_f32(total_v0, p0);
                if vmaxvq_f32(p0) > cur_best {
                    let mut arr = [0.0f32; 4];
                    vst1q_f32(arr.as_mut_ptr(), p0);
                    for (lane, &power) in arr.iter().enumerate() {
                        if power > cur_best {
                            cur_best = power;
                            cur_peak = lag_base + (idx * 4 + lane) as isize;
                        }
                    }
                }
            }
        }
        let mut total = unsafe { vaddvq_f32(vaddq_f32(total_v0, total_v1)) };
        let tail_base = chunks.len() * 4;
        for (idx, value) in tail.iter().enumerate() {
            let power = value.norm_sqr();
            total += power;
            if power > cur_best {
                cur_best = power;
                cur_peak = lag_base + (tail_base + idx) as isize;
            }
        }
        *best = cur_best;
        *peak = cur_peak;
        total
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        let mut total = 0.0f32;
        for (idx, value) in slice.iter().enumerate() {
            let power = value.norm_sqr();
            total += power;
            if power > *best {
                *best = power;
                *peak = lag_base + idx as isize;
            }
        }
        total
    }
}

fn scan_lags(cross: &[Complex<f32>], frame: usize) -> (isize, f32, f32) {
    let size = cross.len();
    let mut peak = 0isize;
    let mut best = 0.0f32;
    let t0 = scan_slice(
        &cross[size - frame..size],
        -(frame as isize),
        &mut peak,
        &mut best,
    );
    let t1 = scan_slice(&cross[..frame], 0, &mut peak, &mut best);
    (peak, best, t0 + t1)
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;
    use crate::testutil::complex_noise;

    fn chirp(len: usize, offset: f32) -> Vec<Complex<f32>> {
        (0..len)
            .map(|k| {
                let t = k as f32 + offset;
                Complex::from_polar(1.0, TAU * (0.02 * t + 0.000_1 * t * t))
            })
            .collect()
    }

    fn noisy(samples: &[Complex<f32>], power: f32, seed: u64) -> Vec<Complex<f32>> {
        samples
            .iter()
            .zip(complex_noise(samples.len(), power, seed))
            .map(|(s, n)| s + n)
            .collect()
    }

    #[test]
    fn an_integer_delay_lands_on_its_own_lag() {
        let mut xcorr = XCorr::new(1024);
        let a = chirp(1024, 0.0);
        for delay in [0isize, 1, 7, 64, -13] {
            let b = chirp(1024, -delay as f32);
            let estimate = xcorr.estimate(&a, &b);
            assert!(
                (estimate.delay_samples - delay as f32).abs() < 0.05,
                "delay {delay}: measured {}",
                estimate.delay_samples
            );
            assert!(estimate.coherence > 0.8, "{estimate:?}");
        }
    }

    #[test]
    fn a_fractional_delay_is_recovered_to_a_tenth_of_a_sample() {
        let mut xcorr = XCorr::new(2048);
        let a = chirp(2048, 0.0);
        for delay in [0.25f32, 0.5, -0.75, 2.4] {
            let b = chirp(2048, -delay);
            let estimate = xcorr.estimate(&a, &b);
            assert!(
                (estimate.delay_samples - delay).abs() < 0.1,
                "delay {delay}: measured {}",
                estimate.delay_samples
            );
        }
    }

    #[test]
    fn a_half_sample_delay_keeps_its_coherence() {
        let mut xcorr = XCorr::new(2048);
        let a = chirp(2048, 0.0);
        for delay in [0.0f32, 0.5, -0.5, 3.5] {
            let estimate = xcorr.estimate(&a, &chirp(2048, -delay));
            assert!(estimate.coherence > 0.9, "delay {delay}: {estimate:?}");
        }
    }

    #[test]
    fn realistic_noise_still_leaves_the_delay_within_a_tenth_of_a_sample() {
        let mut xcorr = XCorr::new(4096);
        let clean = chirp(4096, 0.0);
        let a = noisy(&clean, 0.06, 0x1234);
        let b = noisy(&chirp(4096, -3.0), 0.06, 0x9876);
        let estimate = xcorr.estimate(&a, &b);
        assert!(
            (estimate.delay_samples - 3.0).abs() < 0.1,
            "measured {}",
            estimate.delay_samples
        );
        assert!(estimate.coherence > 0.5, "{estimate:?}");
    }

    #[test]
    fn a_phase_and_gain_offset_come_back_as_measured() {
        let mut xcorr = XCorr::new(1024);
        let a = chirp(1024, 0.0);
        let rotation = Complex::from_polar(0.5f32, 1.1);
        let b: Vec<Complex<f32>> = a.iter().map(|s| s * rotation).collect();
        let estimate = xcorr.estimate(&a, &b);
        assert!(estimate.delay_samples.abs() < 0.05);
        assert!((estimate.phase_rad - 1.1).abs() < 0.02, "{estimate:?}");
        assert!((estimate.gain - 0.5).abs() < 0.02, "{estimate:?}");
        assert!(estimate.coherence > 0.99, "{estimate:?}");
    }

    #[test]
    fn broadband_noise_correlates_to_a_sharp_peak() {
        let mut xcorr = XCorr::new(2048);
        let a = noisy(&vec![Complex::default(); 2048], 1.0, 0x3333);
        let mut b = vec![Complex::default(); 7];
        b.extend_from_slice(&a[..2048 - 7]);
        let estimate = xcorr.estimate(&a, &b);
        assert!(estimate.peak_to_floor_db > 20.0, "{estimate:?}");
    }

    #[test]
    fn a_steady_tone_correlates_everywhere_and_so_has_no_sharp_peak() {
        let mut xcorr = XCorr::new(2048);
        let tone: Vec<Complex<f32>> = (0..2048)
            .map(|k| Complex::from_polar(1.0, TAU * 0.05 * k as f32))
            .collect();
        let estimate = xcorr.estimate(&tone, &tone);
        assert!(estimate.coherence > 0.2, "{estimate:?}");
        assert!(estimate.peak_to_floor_db < 20.0, "{estimate:?}");
    }

    #[test]
    fn two_unrelated_signals_report_no_coherence() {
        let mut xcorr = XCorr::new(2048);
        let a = noisy(&vec![Complex::default(); 2048], 1.0, 0x1111);
        let b = noisy(&vec![Complex::default(); 2048], 1.0, 0x2222);
        let estimate = xcorr.estimate(&a, &b);
        assert!(estimate.coherence < 0.05, "{estimate:?}");
    }

    #[test]
    fn silence_is_reported_rather_than_guessed_at() {
        let mut xcorr = XCorr::new(256);
        let quiet = vec![Complex::default(); 256];
        let estimate = xcorr.estimate(&quiet, &chirp(256, 0.0));
        assert_eq!(estimate, DelayEstimate::nothing());
    }

    #[test]
    fn the_correlation_curve_peaks_where_the_estimate_says() {
        let mut xcorr = XCorr::new(512);
        let a = chirp(512, 0.0);
        let b = chirp(512, -9.0);
        let mut curve = Vec::new();
        xcorr.correlate(&a, &b, &mut curve);
        assert_eq!(curve.len(), 1024);
        let peak = curve
            .iter()
            .enumerate()
            .max_by(|x, y| x.1.total_cmp(y.1))
            .map(|(index, _)| index as isize - 512);
        assert_eq!(peak, Some(9));
    }
}

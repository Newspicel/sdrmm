use std::f64::consts::TAU;

use num_complex::Complex;

use crate::math::complex::widen;

const REBUILD_SAMPLES: usize = 1_024;

/// A sample a [`ToneCorrelator`] can take: real audio or complex baseband.
pub trait Correlated: Copy + Default {
    fn widen(self) -> Complex<f64>;
}

impl Correlated for f32 {
    fn widen(self) -> Complex<f64> {
        Complex::new(f64::from(self), 0.0)
    }
}

impl Correlated for Complex<f32> {
    fn widen(self) -> Complex<f64> {
        widen(self)
    }
}

#[derive(Clone, Debug)]
pub struct ToneCorrelator<T = f32> {
    rot: Complex<f64>,
    exit: Complex<f64>,
    norm: f32,
    buf: Vec<T>,
    pos: usize,
    acc: Complex<f64>,
    since_rebuild: usize,
    rebuild_every: usize,
}

impl ToneCorrelator<f32> {
    #[must_use]
    pub fn new(sample_rate: f64, freq_hz: f64, window: usize) -> Self {
        assert!(sample_rate > 0.0, "sample rate must be positive");
        assert!(
            freq_hz > 0.0 && freq_hz < sample_rate / 2.0,
            "freq must lie inside the Nyquist band"
        );
        Self::build(sample_rate, freq_hz, window, 2.0)
    }

    /// Computes `plus.push(s) - minus.push(s)` across `input`, batching the square roots.
    pub fn process_pair_diff(plus: &mut Self, minus: &mut Self, input: &[f32], out: &mut Vec<f32>) {
        out.clear();
        out.reserve(input.len());
        let p_norm = plus.norm;
        let m_norm = minus.norm;
        let mut idx = 0;
        let mut p_sq = [0.0f32; 4];
        let mut m_sq = [0.0f32; 4];
        while idx + 4 <= input.len() {
            for k in 0..4 {
                let s = input[idx + k];
                p_sq[k] = plus.step_norm_sqr(s) as f32;
                m_sq[k] = minus.step_norm_sqr(s) as f32;
            }
            let diff = sqrt_diff_4(p_sq, p_norm, m_sq, m_norm);
            out.extend_from_slice(&diff);
            idx += 4;
        }
        for &s in &input[idx..] {
            out.push(plus.push(s) - minus.push(s));
        }
    }

    #[inline(always)]
    fn step_norm_sqr(&mut self, sample: f32) -> f64 {
        let leaving = self.buf[self.pos];
        self.buf[self.pos] = sample;
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
        let s = f64::from(sample);
        let l = f64::from(leaving);
        let re = self.acc.re * self.rot.re - self.acc.im * self.rot.im + s - self.exit.re * l;
        let im = self.acc.re * self.rot.im + self.acc.im * self.rot.re - self.exit.im * l;
        self.acc = Complex::new(re, im);
        self.since_rebuild += 1;
        if self.since_rebuild >= self.rebuild_every {
            self.rebuild();
        }
        self.acc.norm_sqr()
    }
}

#[inline(always)]
fn sqrt_diff_4(p_sq: [f32; 4], p_norm: f32, m_sq: [f32; 4], m_norm: f32) -> [f32; 4] {
    #[cfg(target_arch = "aarch64")]
    {
        use std::arch::aarch64::{
            vdupq_n_f32, vld1q_f32, vmulq_f32, vsqrtq_f32, vst1q_f32, vsubq_f32,
        };
        let mut out = [0.0f32; 4];
        unsafe {
            let p = vmulq_f32(vsqrtq_f32(vld1q_f32(p_sq.as_ptr())), vdupq_n_f32(p_norm));
            let m = vmulq_f32(vsqrtq_f32(vld1q_f32(m_sq.as_ptr())), vdupq_n_f32(m_norm));
            vst1q_f32(out.as_mut_ptr(), vsubq_f32(p, m));
        }
        out
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        std::array::from_fn(|k| p_sq[k].sqrt() * p_norm - m_sq[k].sqrt() * m_norm)
    }
}

impl ToneCorrelator<Complex<f32>> {
    #[must_use]
    pub fn complex(sample_rate: f64, freq_hz: f64, window: usize) -> Self {
        assert!(sample_rate > 0.0, "sample rate must be positive");
        assert!(
            freq_hz.abs() < sample_rate / 2.0,
            "freq must lie inside the Nyquist band"
        );
        Self::build(sample_rate, -freq_hz, window, 1.0)
    }
}

impl<T: Correlated> ToneCorrelator<T> {
    fn build(sample_rate: f64, freq_hz: f64, window: usize, gain: f64) -> Self {
        assert!(window >= 2, "window must hold at least two samples");
        let omega = TAU * freq_hz / sample_rate;
        Self {
            rot: Complex::from_polar(1.0, -omega),
            exit: Complex::from_polar(1.0, -omega * window as f64),
            norm: (gain / window as f64) as f32,
            buf: vec![T::default(); window],
            pos: 0,
            acc: Complex::new(0.0, 0.0),
            since_rebuild: 0,
            rebuild_every: window.max(REBUILD_SAMPLES),
        }
    }

    pub fn push(&mut self, sample: T) -> f32 {
        let leaving = self.buf[self.pos];
        self.buf[self.pos] = sample;
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
        self.acc = self.acc * self.rot + sample.widen() - self.exit * leaving.widen();
        self.since_rebuild += 1;
        if self.since_rebuild >= self.rebuild_every {
            self.rebuild();
        }
        self.acc.norm_sqr().sqrt() as f32 * self.norm
    }

    #[must_use]
    pub fn correlation(&self) -> Complex<f64> {
        self.acc
    }

    pub fn reset(&mut self) {
        self.buf.fill(T::default());
        self.pos = 0;
        self.acc = Complex::new(0.0, 0.0);
        self.since_rebuild = 0;
    }

    fn rebuild(&mut self) {
        let (recent, earlier) = self.buf.split_at(self.pos);
        let mut acc = Complex::new(0.0, 0.0);
        let mut phasor = Complex::new(1.0, 0.0);
        for sample in recent.iter().rev().chain(earlier.iter().rev()) {
            acc += phasor * sample.widen();
            phasor *= self.rot;
        }
        self.acc = acc;
        self.since_rebuild = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{math::db::Decibels, testutil::real_tone};

    const RATE: f64 = 48_000.0;
    const MARK_HZ: f64 = 1_200.0;
    const SPACE_HZ: f64 = 2_200.0;

    fn correlate(tone_hz: f64, detector_hz: f64, window: usize) -> f32 {
        let mut c = ToneCorrelator::new(RATE, detector_hz, window);
        let x = real_tone(tone_hz / RATE, window * 20);
        let mut peak = 0.0f32;
        for (n, &s) in x.iter().enumerate() {
            let m = c.push(s);
            if n >= window * 4 {
                peak = peak.max(m);
            }
        }
        peak
    }

    #[test]
    fn bell202_tones_separate_by_more_than_20_db() {
        let window = 48;
        for (tone, other) in [(MARK_HZ, SPACE_HZ), (SPACE_HZ, MARK_HZ)] {
            let matched = correlate(tone, tone, window);
            let rejected = correlate(tone, other, window);
            let separation = (matched / rejected).amplitude_to_db();
            assert!(
                separation > 20.0,
                "{tone} Hz: matched {matched}, rejected {rejected} ({separation} dB)"
            );
        }
    }

    #[test]
    fn sliding_recurrence_stays_accurate_over_200k_samples() {
        let window = 48;
        let x = real_tone(1_700.0 / RATE, 200_000 + window);
        let mut aged = ToneCorrelator::new(RATE, MARK_HZ, window);
        for &s in &x[..200_000] {
            aged.push(s);
        }
        let mut fresh = ToneCorrelator::new(RATE, MARK_HZ, window);
        let (mut a, mut b) = (0.0, 0.0);
        for &s in &x[200_000..] {
            a = aged.push(s);
            b = fresh.push(s);
        }
        assert!((a - b).abs() < 1e-5, "drifted: aged {a} vs fresh {b}");
    }

    #[test]
    fn the_complex_correlator_tells_a_tone_from_its_mirror() {
        let tone: Vec<Complex<f32>> = (0..4_800)
            .map(|n| Complex::from_polar(1.0, (TAU * 3_000.0 * f64::from(n) / RATE) as f32))
            .collect();
        let mut upper = ToneCorrelator::complex(RATE, 3_000.0, 160);
        let mut lower = ToneCorrelator::complex(RATE, -3_000.0, 160);
        let (mut a, mut b) = (0.0, 0.0);
        for &sample in &tone {
            a = upper.push(sample);
            b = lower.push(sample);
        }
        assert!((a - 1.0).abs() < 1e-3, "matched {a}");
        assert!(b < 1e-3, "mirror {b}");
    }
}

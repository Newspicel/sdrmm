mod costas;
mod lock;
mod loop_filter;
mod phase;

use std::f64::consts::{PI, TAU};

use num_complex::Complex;

pub(crate) use self::loop_filter::loop_gains;
pub use self::{costas::Costas, loop_filter::LoopFilter};
use self::{
    lock::LockDetector,
    phase::{phase_error, phasor, step_phase, wrap_pi},
};

#[derive(Clone, Debug)]
pub struct Pll {
    filter: LoopFilter,
    lock: LockDetector,
    center: f64,
    phase: f64,
    inc: f64,
}

impl Pll {
    #[must_use]
    pub fn new(loop_bw: f64, damping: f64, center_norm: f64, range_norm: f64) -> Self {
        Self {
            filter: LoopFilter::new(loop_bw, damping, range_norm),
            lock: LockDetector::new(loop_bw, PI),
            center: TAU * center_norm,
            phase: 0.0,
            inc: 0.0,
        }
    }

    #[inline]
    #[must_use]
    pub fn process(&mut self, sample: Complex<f32>) -> Complex<f32> {
        self.phase = step_phase(self.phase, self.inc);
        let reference = phasor(self.phase);
        let error = phase_error(sample * reference.conj());
        self.lock.update(error);
        self.inc = wrap_pi(self.center + self.filter.advance(error.unwrap_or(0.0)));
        reference
    }

    #[inline]
    #[must_use]
    pub fn harmonic(&self, n: f64) -> Complex<f32> {
        phasor(n * self.phase)
    }

    #[must_use]
    pub fn freq_norm(&self) -> f64 {
        self.center / TAU + self.filter.freq_norm()
    }

    #[must_use]
    pub fn increment_norm(&self) -> f64 {
        self.inc / TAU
    }

    #[must_use]
    pub fn lock(&self) -> f32 {
        self.lock.value()
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_1_SQRT_2;

    use super::*;
    use crate::{math::complex::widen, testutil::complex_tone};

    const DAMPING: f64 = FRAC_1_SQRT_2;
    const PILOT: f64 = 19_000.0 / 240_000.0;
    const RANGE: f64 = 300.0 / 240_000.0;
    const PILOT_BW: f64 = 0.005;

    fn pilot_pll() -> Pll {
        Pll::new(PILOT_BW, DAMPING, PILOT, RANGE)
    }

    #[test]
    fn pll_locks_to_a_tone_inside_the_pull_in_range() {
        let tone = PILOT + 120.0 / 240_000.0;
        let mut pll = pilot_pll();
        for x in complex_tone(tone, 40_000) {
            let _ = pll.process(x);
        }
        let err = (pll.freq_norm() - tone).abs();
        assert!(
            err < 0.01 * (tone - PILOT),
            "settled at {} instead of {tone}",
            pll.freq_norm()
        );
        assert!(pll.lock() > 0.5, "lock {}", pll.lock());
    }

    #[test]
    fn pll_refuses_to_follow_a_tone_outside_the_range() {
        let tone = PILOT + 3.0 * RANGE;
        let mut pll = pilot_pll();
        for (n, x) in complex_tone(tone, 40_000).into_iter().enumerate() {
            let _ = pll.process(x);
            let offset = pll.freq_norm() - PILOT;
            assert!(offset.abs() <= RANGE + 1e-12, "escaped by {offset} at {n}");
        }
        assert!(
            (pll.freq_norm() - (PILOT + RANGE)).abs() < 1e-9,
            "not clamped at the edge: {}",
            pll.freq_norm()
        );
    }

    #[test]
    fn pll_harmonic_rotates_at_a_multiple_of_the_loop_frequency() {
        let tone = PILOT + 50.0 / 240_000.0;
        let signal = complex_tone(tone, 60_000);
        let mut pll = pilot_pll();
        for &x in &signal[..40_000] {
            let _ = pll.process(x);
        }
        assert!(pll.lock() > 0.5, "lock {}", pll.lock());

        let mut sum = Complex::new(0.0f64, 0.0);
        for (k, &x) in signal[40_000..].iter().enumerate() {
            let _ = pll.process(x);
            let reference = Complex::from_polar(1.0, TAU * 3.0 * tone * (40_000 + k) as f64);
            sum += widen(pll.harmonic(3.0)) * reference.conj();
        }
        let mean = sum / (signal.len() - 40_000) as f64;
        assert!(
            (mean.norm() - 1.0).abs() < 1e-2,
            "harmonic drifted: mean norm {}",
            mean.norm()
        );
    }

    #[test]
    fn pll_recovers_after_a_non_finite_sample() {
        let tone = PILOT + 60.0 / 240_000.0;
        let mut pll = pilot_pll();
        for (n, x) in complex_tone(tone, 40_000).into_iter().enumerate() {
            let x = if n == 100 {
                Complex::new(f32::NAN, 0.0)
            } else {
                x
            };
            assert!(pll.process(x).is_finite(), "reference poisoned at {n}");
        }
        assert!(
            (pll.freq_norm() - tone).abs() < 0.01 * (tone - PILOT),
            "loop stuck at {}",
            pll.freq_norm()
        );
        assert!(pll.lock() > 0.5, "lock {}", pll.lock());
    }
}

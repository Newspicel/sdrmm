use super::one_pole_coeff;

/// Microseconds to seconds.
const SECONDS_PER_US: f64 = 1e-6;

#[derive(Clone, Debug)]
pub struct Deemphasis {
    state: f32,
    coeff: f32,
}

impl Deemphasis {
    #[must_use]
    pub fn new(rate: f64, tau_us: f32) -> Self {
        assert!(rate > 0.0 && tau_us > 0.0, "rate and tau must be positive");
        Self {
            state: 0.0,
            coeff: one_pole_coeff(rate, f64::from(tau_us) * SECONDS_PER_US),
        }
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        if !self.state.is_finite() {
            self.state = 0.0;
        }
        for s in samples {
            self.state += self.coeff * (*s - self.state);
            *s = self.state;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_1_SQRT_2;

    use super::*;
    use crate::testutil::{real_tone, rms_r};

    fn deemphasis_gain(rate: f64, tau_us: f32, freq_hz: f64) -> f32 {
        let mut filter = Deemphasis::new(rate, tau_us);
        let n = (rate * 0.2) as usize;
        let mut x = real_tone(freq_hz / rate, n);
        filter.process(&mut x);
        rms_r(&x[n / 2..]) / FRAC_1_SQRT_2
    }

    #[test]
    fn deemphasis_minus_3_db_point_matches_tau() {
        let (rate, tau_us) = (48_000.0, 50.0f32);
        let expected = 1.0 / (std::f64::consts::TAU * 50e-6);
        let (mut lo, mut hi) = (1_000.0f64, 10_000.0f64);
        for _ in 0..25 {
            let mid = f64::midpoint(lo, hi);
            if deemphasis_gain(rate, tau_us, mid) > FRAC_1_SQRT_2 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let found = f64::midpoint(lo, hi);
        let rel_err = (found - expected).abs() / expected;
        assert!(
            rel_err < 0.05,
            "-3 dB at {found} Hz, expected {expected} Hz"
        );
    }

    #[test]
    fn deemphasis_recovers_after_non_finite_sample() {
        let mut filter = Deemphasis::new(48_000.0, 50.0);
        let mut poisoned = real_tone(1_000.0 / 48_000.0, 480);
        poisoned[100] = f32::NAN;
        filter.process(&mut poisoned);

        let mut tone = real_tone(1_000.0 / 48_000.0, 9_600);
        filter.process(&mut tone);
        assert!(tone.iter().all(|v| v.is_finite()), "state still poisoned");
        let gain = rms_r(&tone[4_800..]) / FRAC_1_SQRT_2;
        assert!((0.9..1.0).contains(&gain), "post-recovery gain {gain}");
    }
}

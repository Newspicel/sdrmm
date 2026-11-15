use num_complex::Complex;

use super::corner_coeff;

#[derive(Clone, Debug)]
pub struct ComplexOnePole {
    stages: Vec<Complex<f32>>,
    coeff: f32,
}

impl ComplexOnePole {
    #[must_use]
    pub fn new(rate: f64, cutoff_hz: f64, stages: usize) -> Self {
        assert!(
            rate > 0.0 && cutoff_hz > 0.0 && stages > 0,
            "rate, cutoff and stage count must be positive"
        );
        Self {
            stages: vec![Complex::new(0.0, 0.0); stages],
            coeff: corner_coeff(rate, cutoff_hz),
        }
    }

    #[must_use]
    pub fn process(&mut self, sample: Complex<f32>) -> Complex<f32> {
        let mut v = if sample.is_finite() {
            sample
        } else {
            Complex::new(0.0, 0.0)
        };
        for stage in &mut self.stages {
            *stage += (v - *stage) * self.coeff;
            v = *stage;
        }
        v
    }

    pub fn reset(&mut self) {
        for stage in &mut self.stages {
            *stage = Complex::new(0.0, 0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{complex_tone, rms_c};

    fn cascade_gain(freq_norm: f64, stages: usize) -> f32 {
        let mut filter = ComplexOnePole::new(1.0, CUTOFF_NORM, stages);
        let input = complex_tone(freq_norm, 200_000);
        let out: Vec<Complex<f32>> = input.iter().map(|&s| filter.process(s)).collect();
        rms_c(&out[out.len() / 2..])
    }

    const CUTOFF_NORM: f64 = 0.001;

    #[test]
    fn complex_one_pole_matches_the_analytic_single_pole_response() {
        for (freq_norm, stages) in [(CUTOFF_NORM, 1), (10.0 * CUTOFF_NORM, 1), (0.01, 3)] {
            let ratio = freq_norm / CUTOFF_NORM;
            let expected = (1.0 + ratio * ratio).powf(-(stages as f64) / 2.0) as f32;
            let gain = cascade_gain(freq_norm, stages);
            assert!(
                (gain - expected).abs() < 0.1 * expected.max(1e-3),
                "{stages} stages at {ratio}·fc: gain {gain}, expected {expected}"
            );
        }
    }

    #[test]
    fn complex_one_pole_is_symmetric_about_dc_and_passes_it_unrotated() {
        let above = cascade_gain(0.005, 3);
        let below = cascade_gain(-0.005, 3);
        assert!((above - below).abs() < 1e-4, "{above} vs {below}");

        let mut filter = ComplexOnePole::new(1.0, CUTOFF_NORM, 3);
        let dc = Complex::new(0.6f32, -0.8);
        let mut out = Complex::new(0.0, 0.0);
        for _ in 0..20_000 {
            out = filter.process(dc);
        }
        assert!(
            (out - dc).norm() < 1e-3,
            "dc settled to {out}, expected {dc}"
        );
    }

    #[test]
    fn complex_one_pole_recovers_after_non_finite_sample() {
        let mut filter = ComplexOnePole::new(1.0, CUTOFF_NORM, 2);
        let _ = filter.process(Complex::new(f32::NAN, 0.0));
        let _ = filter.process(Complex::new(0.0, f32::INFINITY));
        let mut out = Complex::new(0.0, 0.0);
        for _ in 0..20_000 {
            out = filter.process(Complex::new(1.0, 0.0));
        }
        assert!(out.re.is_finite() && out.im.is_finite(), "state poisoned");
        assert!((out.re - 1.0).abs() < 1e-3, "post-recovery gain {}", out.re);
    }
}

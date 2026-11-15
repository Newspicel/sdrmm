use std::f64::consts::TAU;

#[derive(Clone, Debug)]
pub struct LoopFilter {
    alpha: f64,
    beta: f64,
    freq: f64,
    limit: f64,
}

impl LoopFilter {
    #[must_use]
    pub fn new(loop_bw: f64, damping: f64, freq_limit_norm: f64) -> Self {
        assert!(
            loop_bw > 0.0 && damping > 0.0 && freq_limit_norm >= 0.0,
            "loop filter parameters must be positive"
        );
        let (alpha, beta) = loop_gains(loop_bw, damping);
        Self {
            alpha,
            beta,
            freq: 0.0,
            limit: TAU * freq_limit_norm,
        }
    }

    #[must_use]
    pub fn advance(&mut self, error: f64) -> f64 {
        let error = if error.is_finite() { error } else { 0.0 };
        self.freq = (self.freq + self.beta * error).clamp(-self.limit, self.limit);
        self.freq + self.alpha * error
    }

    #[must_use]
    pub fn freq_norm(&self) -> f64 {
        self.freq / TAU
    }

    pub fn reset(&mut self, freq_norm: f64) {
        self.freq = (TAU * freq_norm).clamp(-self.limit, self.limit);
    }
}

/// Proportional and integral gains of a second-order loop with natural frequency `loop_bw`
/// and the given damping.
pub(crate) fn loop_gains(loop_bw: f64, damping: f64) -> (f64, f64) {
    let wn = loop_bw;
    let d = 1.0 + 2.0 * damping * wn + wn * wn;
    (4.0 * damping * wn / d, 4.0 * wn * wn / d)
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_1_SQRT_2;

    use super::*;

    const DAMPING: f64 = FRAC_1_SQRT_2;

    #[test]
    fn loop_filter_integrator_clamps_and_resets() {
        let limit = 0.002;
        let mut f = LoopFilter::new(0.01, DAMPING, limit);
        for _ in 0..10_000 {
            let _ = f.advance(1.0);
        }
        assert!((f.freq_norm() - limit).abs() < 1e-12, "{}", f.freq_norm());
        for _ in 0..10_000 {
            let _ = f.advance(-1.0);
        }
        assert!((f.freq_norm() + limit).abs() < 1e-12, "{}", f.freq_norm());

        f.reset(0.0);
        assert!(f.freq_norm().abs() < f64::EPSILON);
        assert!(f.advance(0.0).abs() < f64::EPSILON, "zero error must coast");
        f.reset(10.0 * limit);
        assert!(
            (f.freq_norm() - limit).abs() < 1e-12,
            "reset ignored the limit: {}",
            f.freq_norm()
        );
    }

    #[test]
    fn loop_filter_ignores_a_non_finite_error() {
        let mut f = LoopFilter::new(0.01, DAMPING, 0.01);
        for _ in 0..100 {
            let _ = f.advance(0.5);
        }
        let before = f.freq_norm();
        assert!(f.advance(f64::NAN).is_finite(), "NaN reached the output");
        assert!((f.freq_norm() - before).abs() < f64::EPSILON);
    }
}

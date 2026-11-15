use num_complex::Complex;

use super::CORRELATION_LAGS;

/// How many range cells one independent noise sample spans, from the reference
/// autocorrelation over `CORRELATION_LAGS` lags, tapered over a run of `run` cells.
#[must_use]
pub fn range_correlation(autocorrelation: &[Complex<f32>], run: usize) -> f64 {
    let Some(zero) = autocorrelation.first().map(|value| f64::from(value.norm())) else {
        return 1.0;
    };
    if !(zero > 0.0 && zero.is_finite() && run > 0) {
        return 1.0;
    }
    let tail: f64 = autocorrelation
        .iter()
        .enumerate()
        .skip(1)
        .take(CORRELATION_LAGS)
        .map(|(lag, value)| {
            let ratio = f64::from(value.norm()) / zero;
            let weight = (1.0 - lag as f64 / run as f64).max(0.0);
            weight * ratio * ratio
        })
        .sum();
    if tail.is_finite() {
        1.0 + 2.0 * tail
    } else {
        1.0
    }
}

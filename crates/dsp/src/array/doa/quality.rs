use std::f64::consts::SQRT_2;

use super::{CONFIDENCE_WINDOW_DEG, SIGMA_FLOOR_DEG};
use crate::math::special::erf;

pub const MAX_SIGMA_DEG: f64 = 180.0;

const MIN_SPREAD: f64 = 1e-12;
const MIN_SNR: f64 = 1e-12;
const MISMATCH_SPAN_DEG: f32 = 90.0;

#[must_use]
pub fn rate_spread(rates: &[f64]) -> f64 {
    if rates.is_empty() {
        return 0.0;
    }
    let mean = rates.iter().sum::<f64>() / rates.len() as f64;
    rates.iter().map(|g| (g - mean) * (g - mean)).sum()
}

#[must_use]
pub fn crb_sigma_rad(rates: &[f64], snr: f64, elements: usize, snapshots: f64) -> f64 {
    let spread = rate_spread(rates).max(MIN_SPREAD);
    let snr = if snr.is_finite() {
        snr.max(MIN_SNR)
    } else {
        MIN_SNR
    };
    let n = elements.max(1) as f64;
    let variance = (1.0 / snr + 1.0 / (n * snr * snr)) / (2.0 * snapshots.max(1.0) * spread);
    variance.sqrt()
}

#[must_use]
pub fn model_sigma_rad(rates: &[f64], cal_sigma_deg: f64) -> f64 {
    cal_sigma_deg.abs().to_radians() / rate_spread(rates).max(MIN_SPREAD).sqrt()
}

#[must_use]
pub fn total_sigma_deg(crb_rad: f64, model_rad: f64) -> f64 {
    let crb = crb_rad.to_degrees();
    let model = model_rad.to_degrees();
    let total = (crb * crb + model * model + SIGMA_FLOOR_DEG * SIGMA_FLOOR_DEG).sqrt();
    if total.is_finite() {
        total.min(MAX_SIGMA_DEG)
    } else {
        MAX_SIGMA_DEG
    }
}

#[must_use]
pub fn confidence(sigma_deg: f64, ambiguity: f32) -> f32 {
    if !(sigma_deg.is_finite() && sigma_deg > 0.0) {
        return 0.0;
    }
    let inside = erf(CONFIDENCE_WINDOW_DEG / (sigma_deg * SQRT_2));
    let unique = f64::from(1.0 - ambiguity.clamp(0.0, 1.0));
    (inside * unique).clamp(0.0, 1.0) as f32
}

#[must_use]
pub fn mismatch_share(fit: f32, trace: f32, noise: f32, elements: usize, sources: usize) -> f32 {
    let (trace, noise) = (f64::from(trace), f64::from(noise));
    let signal = trace - elements as f64 * noise;
    if sources == 0 || signal.is_nan() || signal <= 0.0 {
        return 0.0;
    }
    let unexplained =
        (1.0 - f64::from(fit)) * trace - elements.saturating_sub(sources) as f64 * noise;
    (unexplained / signal).clamp(0.0, 1.0) as f32
}

#[must_use]
pub fn mismatch_sigma_deg(sigma_deg: f32, share: f32) -> f32 {
    sigma_deg
        .hypot(MISMATCH_SPAN_DEG * share)
        .min(MAX_SIGMA_DEG as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::doa::testing::rates_at;
    use crate::array::manifold::testing::half_wave_uca5;

    #[test]
    fn crb_shrinks_with_snr_and_snapshots() {
        let rates = rates_at(&half_wave_uca5(), 37.0);
        let low = crb_sigma_rad(&rates, 1.0, 5, 1000.0);
        let high = crb_sigma_rad(&rates, 100.0, 5, 1000.0);
        let longer = crb_sigma_rad(&rates, 1.0, 5, 4000.0);
        assert!(high < low / 5.0);
        assert!((longer - low / 2.0).abs() < 1e-12);
        let spread = rate_spread(&rates);
        let want = ((1.0 + 0.2) / (2000.0 * spread)).sqrt();
        assert!((low - want).abs() < 1e-12);
        assert!(crb_sigma_rad(&rates, f64::NAN, 5, 1000.0).is_finite());
    }

    #[test]
    fn calibration_error_scales_with_the_rate_spread() {
        let rates = rates_at(&half_wave_uca5(), 37.0);
        let model = model_sigma_rad(&rates, 5.0);
        assert!((model - 5f64.to_radians() / rate_spread(&rates).sqrt()).abs() < 1e-12);
        let total = total_sigma_deg(0.0, model);
        assert!(total >= model.to_degrees());
        assert!((total_sigma_deg(0.0, 0.0) - SIGMA_FLOOR_DEG).abs() < 1e-12);
        assert_eq!(total_sigma_deg(f64::INFINITY, 0.0), MAX_SIGMA_DEG);
        assert_eq!(total_sigma_deg(f64::NAN, 0.0), MAX_SIGMA_DEG);
    }

    #[test]
    fn mismatch_counts_power_no_wave_explains() {
        let noise = 1.0;
        let clean_trace = 5.0 * (31.7 + noise);
        let clean_fit = 1.0 - 4.0 * noise / clean_trace;
        assert!(mismatch_share(clean_fit, clean_trace, noise, 5, 1) < 1e-4);
        let merged = mismatch_share(0.67, clean_trace, noise, 5, 1);
        assert!((merged - 0.32).abs() < 0.01, "{merged}");
        assert_eq!(mismatch_share(0.5, 5.0, 1.0, 5, 1), 0.0);
        assert_eq!(mismatch_share(0.5, clean_trace, noise, 5, 0), 0.0);
        assert!((mismatch_sigma_deg(3.0, 0.0) - 3.0).abs() < 1e-6);
        assert!(mismatch_sigma_deg(3.0, 0.32) > 28.0);
        assert_eq!(mismatch_sigma_deg(3.0, 1.0), 90.0f32.hypot(3.0));
    }

    #[test]
    fn confidence_follows_sigma_and_ambiguity() {
        assert!(confidence(2.0, 0.0) > 0.98);
        assert!(confidence(60.0, 0.0) < 0.1);
        assert!((confidence(2.0, 0.5) - confidence(2.0, 0.0) / 2.0).abs() < 1e-6);
        assert_eq!(confidence(f64::NAN, 0.0), 0.0);
    }
}

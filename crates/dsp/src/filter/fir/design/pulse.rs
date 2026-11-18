use std::f64::consts::PI;

use super::{MIN_TAPS, unity_dc_gain};
use crate::math::special::sinc;

/// Closer than this to a removable singularity, a pulse takes its limit.
const SINGULARITY: f64 = 1e-9;
/// The cosine shaping pulse's limit at its singularity, an eighth of a bit from the centre.
const COSINE_PULSE_LIMIT: f64 = 2.0;
/// The biphase pulse's inverted copy follows half a bit behind.
const BIPHASE_DELAY_SYMBOLS: f64 = 0.5;

#[must_use]
pub fn design_gaussian(sps: f64, bt: f64, span: usize) -> Vec<f32> {
    assert!(sps > 1.0, "need more than one sample per symbol");
    assert!(bt > 0.0, "bandwidth-time product must be positive");
    assert!(span >= 1, "span must cover at least one symbol");
    let taps = odd((span as f64 * sps).round() as usize);
    assert!(taps >= MIN_TAPS, "need at least 3 taps");
    let sigma = (2.0f64.ln()).sqrt() / (2.0 * PI * bt);
    let center = (taps - 1) as f64 / 2.0;
    unity_dc_gain(
        (0..taps)
            .map(|k| {
                let t = (k as f64 - center) / sps;
                (-t * t / (2.0 * sigma * sigma)).exp()
            })
            .collect(),
    )
}

/// Root-raised-cosine over `span` symbols each side, scaled to unity DC gain.
#[must_use]
pub fn design_rrc(sps: f64, alpha: f64, span: usize) -> Vec<f32> {
    assert!(span >= 1, "span must cover at least one symbol");
    let taps = odd((2.0 * span as f64 * sps).round() as usize);
    unity_dc_gain(rrc_taps(sps, alpha, taps))
}

/// Root-raised-cosine with an exact tap count, scaled to unit energy rather than unity DC gain,
/// so a matched filter's output SNR does not depend on how many taps it has.
#[must_use]
pub fn design_rrc_unit_energy(sps: f64, alpha: f64, taps: usize) -> Vec<f32> {
    let h = rrc_taps(sps, alpha, taps);
    let energy = h.iter().map(|v| v * v).sum::<f64>().sqrt();
    h.into_iter().map(|v| (v / energy) as f32).collect()
}

#[must_use]
pub fn design_cosine_shaping(sps: f64, span: usize) -> Vec<f32> {
    unit_abs_sum(&cosine_taps(sps, span, false))
}

#[must_use]
pub fn design_cosine_biphase(sps: f64, span: usize) -> Vec<f32> {
    unit_abs_sum(&cosine_taps(sps, span, true))
}

fn odd(taps: usize) -> usize {
    if taps.is_multiple_of(2) {
        taps + 1
    } else {
        taps
    }
}

fn rrc_taps(sps: f64, alpha: f64, taps: usize) -> Vec<f64> {
    assert!(sps > 1.0, "need more than one sample per symbol");
    assert!(
        alpha > 0.0 && alpha <= 1.0,
        "roll-off must be in (0, 1]: {alpha}"
    );
    let center = taps.saturating_sub(1) as f64 / 2.0;
    (0..taps)
        .map(|k| rrc_pulse((k as f64 - center) / sps, alpha))
        .collect()
}

/// Root-raised-cosine pulse at `t` symbols with roll-off `alpha`; a sinc when `alpha` is zero.
#[must_use]
pub fn rrc_pulse(t: f64, alpha: f64) -> f64 {
    if alpha <= 0.0 {
        return sinc(t);
    }
    if t.abs() < SINGULARITY {
        return 1.0 - alpha + 4.0 * alpha / PI;
    }
    if (t.abs() - 1.0 / (4.0 * alpha)).abs() < SINGULARITY {
        let x = PI / (4.0 * alpha);
        return alpha / 2.0f64.sqrt() * ((1.0 + 2.0 / PI) * x.sin() + (1.0 - 2.0 / PI) * x.cos());
    }
    let pt = PI * t;
    let numerator = ((1.0 - alpha) * pt).sin() + 4.0 * alpha * t * ((1.0 + alpha) * pt).cos();
    numerator / (pt * (1.0 - (4.0 * alpha * t).powi(2)))
}

fn cosine_taps(sps: f64, span: usize, biphase: bool) -> Vec<f64> {
    assert!(sps > 1.0, "need more than one sample per symbol");
    assert!(span >= 1, "span must cover at least one symbol");
    let half = (span as f64 * sps).round().max(1.0) as usize;
    (0..=2 * half)
        .map(|k| {
            let t = (k as f64 - half as f64) / sps;
            if biphase {
                cosine_pulse(t) - cosine_pulse(t - BIPHASE_DELAY_SYMBOLS)
            } else {
                cosine_pulse(t)
            }
        })
        .collect()
}

fn cosine_pulse(symbols: f64) -> f64 {
    let denom = 2.0 * PI * (1.0 / 16.0 - 4.0 * symbols * symbols);
    if denom.abs() < SINGULARITY {
        return COSINE_PULSE_LIMIT;
    }
    (4.0 * PI * symbols).cos() / denom
}

fn unit_abs_sum(taps: &[f64]) -> Vec<f32> {
    let scale = taps
        .iter()
        .map(|v| v.abs())
        .sum::<f64>()
        .max(f64::MIN_POSITIVE);
    taps.iter().map(|v| (v / scale) as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::{super::response_db, *};

    const SPS: f64 = 9_600.0 / 1_187.5;

    #[test]
    fn unit_energy_rrc_has_unit_energy_and_the_shape_of_the_unity_gain_one() {
        let h = design_rrc_unit_energy(8.0, 0.35, 81);
        let energy: f64 = h.iter().map(|&v| f64::from(v).powi(2)).sum();
        assert!((energy - 1.0).abs() < 1e-6, "energy {energy}");
        let reference = design_rrc(8.0, 0.35, 5);
        assert_eq!(reference.len(), h.len());
        let scale = h[40] / reference[40];
        for (a, b) in h.iter().zip(&reference) {
            assert!((a - b * scale).abs() < 1e-6, "{a} vs {b}");
        }
    }

    #[test]
    fn unit_energy_rrc_handles_the_quarter_symbol_singularity() {
        let h = design_rrc_unit_energy(4.0, 1.0, 33);
        assert!(h.iter().all(|v| v.is_finite()));
        assert!(h.iter().zip(h.iter().rev()).all(|(a, b)| a == b));
    }

    #[test]
    fn gaussian_length_rounds_up_to_odd() {
        assert_eq!(design_gaussian(10.0, 0.4, 4).len(), 41);
        assert_eq!(design_gaussian(8.0, 0.4, 3).len(), 25);
        assert_eq!(design_gaussian(5.0, 0.5, 3).len(), 15);
    }

    #[test]
    fn gaussian_is_unit_gain_symmetric_and_unimodal() {
        let h = design_gaussian(10.0, 0.4, 4);
        let sum: f64 = h.iter().map(|&v| f64::from(v)).sum();
        assert!((sum - 1.0).abs() < 1e-6, "dc gain {sum}");

        let mid = h.len() / 2;
        for k in 0..mid {
            assert!(
                (h[k] - h[h.len() - 1 - k]).abs() < 1e-9,
                "asymmetric at tap {k}"
            );
            assert!(
                h[k] < h[k + 1],
                "not increasing toward the center at tap {k}"
            );
        }
        assert!(h[mid] > 0.0, "center tap must be the peak");
    }

    #[test]
    fn cosine_shaping_is_a_cosine_lowpass_that_dies_at_twice_the_bit_rate() {
        let h = design_cosine_shaping(SPS, 5);
        let peak = response_db(&h, 0.0);
        for (symbols, expected_db) in [(0.0, 0.0), (0.5, -0.69), (1.0, -3.01), (1.5, -8.34)] {
            let db = response_db(&h, symbols / SPS) - peak;
            assert!(
                (db - expected_db).abs() < 0.3,
                "{symbols} bit rates: {db} dB, expected {expected_db}"
            );
        }
        assert_band_edge_below_40_db(&h, peak);
    }

    #[test]
    fn cosine_biphase_nulls_dc_and_peaks_at_the_bit_rate() {
        let h = design_cosine_biphase(SPS, 4);
        let peak = response_db(&h, 1.0 / SPS);
        assert!(
            response_db(&h, 0.0) - peak < -60.0,
            "dc leak {} dB",
            response_db(&h, 0.0) - peak
        );
        for symbols in [0.5, 1.5] {
            let db = response_db(&h, symbols / SPS) - peak;
            assert!(db < 0.0, "{symbols} bit rates rose above the peak: {db} dB");
        }
        assert_band_edge_below_40_db(&h, peak);
    }

    fn assert_band_edge_below_40_db(h: &[f32], peak: f64) {
        for i in 0..=40 {
            let symbols = 2.0 + 2.0 * f64::from(i) / 40.0;
            let db = response_db(h, symbols / SPS) - peak;
            assert!(db < -40.0, "band edge leak {db} dB at {symbols} bit rates");
        }
    }

    #[test]
    fn cosine_biphase_is_antisymmetric_about_the_half_symbol() {
        let h = design_cosine_biphase(SPS, 4);
        let sum: f64 = h.iter().map(|&v| f64::from(v)).sum();
        assert!(sum.abs() < 1e-5, "the taps carry a dc component: {sum}");
    }
}

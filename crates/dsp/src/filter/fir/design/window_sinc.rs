use std::f64::consts::PI;

use super::{MIN_TAPS, unity_dc_gain};
use crate::math::special::sinc;

/// Blackman window terms.
const BLACKMAN: [f64; 3] = [0.42, 0.5, 0.08];
/// Four-term Blackman-Harris window terms.
const BLACKMAN_HARRIS: [f64; 4] = [0.35875, 0.48829, 0.14128, 0.01168];
/// Taps per cycle-per-sample of transition a Blackman low-pass needs for its 74 dB stopband.
const BLACKMAN_TRANSITION_TAPS: f64 = 5.5;

/// Windowed-sinc low-pass under a Blackman window. `cutoff` is in cycles per sample.
#[must_use]
pub fn design_lowpass(taps: usize, cutoff: f64) -> Vec<f32> {
    windowed_sinc(taps, cutoff, |k| blackman(k, taps))
}

/// Windowed-sinc low-pass under a symmetric four-term Blackman-Harris window. Its stopband sits
/// near -92 dB, far below [`design_lowpass`]'s Blackman.
#[must_use]
pub fn design_lowpass_blackman_harris(taps: usize, cutoff: f64) -> Vec<f32> {
    windowed_sinc(taps, cutoff, |k| blackman_harris_symmetric(k, taps))
}

#[must_use]
pub fn design_bandpass(taps: usize, low: f64, high: f64) -> Vec<f32> {
    assert!(
        low > 0.0 && low < high && high < 0.5,
        "band edges must satisfy 0 < low < high < 0.5"
    );
    let prototype = design_lowpass(taps, (high - low) / 2.0);
    let (band_center, center) = (f64::midpoint(low, high), (taps - 1) as f64 / 2.0);
    prototype
        .iter()
        .enumerate()
        .map(|(k, &v)| {
            let m = (2.0 * PI * band_center * (k as f64 - center)).cos();
            (2.0 * f64::from(v) * m) as f32
        })
        .collect()
}

/// The tap count [`design_lowpass`] needs for a transition `transition` cycles per sample wide.
pub(crate) fn blackman_taps(transition: f64) -> usize {
    (BLACKMAN_TRANSITION_TAPS / transition).ceil() as usize
}

/// Windowed-sinc low-pass with unity DC gain, `window(k)` weighting tap `k`.
///
/// # Panics
/// Panics unless `taps >= 3` and `cutoff` lies in (0, 0.5).
pub(super) fn windowed_sinc(taps: usize, cutoff: f64, window: impl Fn(usize) -> f64) -> Vec<f32> {
    assert!(taps >= MIN_TAPS, "need at least 3 taps");
    assert!(cutoff > 0.0 && cutoff < 0.5, "cutoff must be in (0, 0.5)");
    let center = (taps - 1) as f64 / 2.0;
    unity_dc_gain(
        (0..taps)
            .map(|k| 2.0 * cutoff * sinc(2.0 * cutoff * (k as f64 - center)) * window(k))
            .collect(),
    )
}

fn blackman(k: usize, n: usize) -> f64 {
    let x = 2.0 * PI * k.min(n - 1 - k) as f64 / (n - 1) as f64;
    BLACKMAN[0] - BLACKMAN[1] * x.cos() + BLACKMAN[2] * (2.0 * x).cos()
}

fn blackman_harris_symmetric(k: usize, n: usize) -> f64 {
    let a = BLACKMAN_HARRIS;
    let x = 2.0 * PI * k as f64 / (n as f64 - 1.0);
    a[0] - a[1] * x.cos() + a[2] * (2.0 * x).cos() - a[3] * (3.0 * x).cos()
}

#[cfg(test)]
mod tests {
    use super::{super::response_db, *};
    use crate::{
        filter::fir::StreamFir,
        testutil::{real_tone, rms_r},
    };

    fn tone_gain(h: &[f32], freq: f64) -> f32 {
        let mut fir = StreamFir::<f32, f32>::new(h, 1);
        let mut out = Vec::new();
        fir.process(&real_tone(freq, 16_384), &mut out);
        rms_r(&out[h.len()..]) * std::f32::consts::SQRT_2
    }

    #[test]
    fn dc_gain_is_unity() {
        let h = design_lowpass(129, 0.1);
        let sum: f64 = h.iter().map(|&v| f64::from(v)).sum();
        assert!((sum - 1.0).abs() < 1e-6, "dc gain {sum}");
    }

    #[test]
    fn passband_ripple_under_half_db() {
        let h = design_lowpass(129, 0.1);
        for i in 0..=150 {
            let f = 0.075 * f64::from(i) / 150.0;
            let db = response_db(&h, f);
            assert!(db.abs() < 0.5, "ripple {db} dB at f={f}");
        }
    }

    #[test]
    fn stopband_below_minus_50_db_beyond_1_5x_cutoff() {
        let h = design_lowpass(129, 0.1);
        for i in 0..=350 {
            let f = 0.15 + (0.5 - 0.15) * f64::from(i) / 350.0;
            let db = response_db(&h, f);
            assert!(db < -50.0, "stopband leak {db} dB at f={f}");
        }
    }

    #[test]
    fn bandpass_passes_its_band_and_rejects_outside() {
        let h = design_bandpass(255, 0.02, 0.06);
        for &f in &[0.031, 0.04, 0.049] {
            let gain = tone_gain(&h, f);
            assert!((0.9..1.1).contains(&gain), "passband gain {gain} at f={f}");
        }
        for &f in &[0.002, 0.008, 0.12, 0.45] {
            let gain = tone_gain(&h, f);
            assert!(gain < 0.01, "stopband gain {gain} at f={f}");
        }
    }

    #[test]
    fn blackman_harris_lowpass_is_unit_gain_symmetric_and_deep_in_the_stopband() {
        let h = design_lowpass_blackman_harris(121, 0.1);
        let dc: f64 = h.iter().map(|&v| f64::from(v)).sum();
        assert!((dc - 1.0).abs() < 1e-5, "dc {dc}");
        assert!(h.iter().zip(h.iter().rev()).all(|(a, b)| a == b));
        assert!(response_db(&h, 0.02) > -0.1);
        assert!(response_db(&h, 0.2) < -90.0, "{}", response_db(&h, 0.2));
    }

    #[test]
    fn lowpass_is_exactly_symmetric() {
        for taps in [3, 4, 11, 64, 127, 255] {
            let h = design_lowpass(taps, 0.11);
            assert!(h.iter().eq(h.iter().rev()), "taps={taps}");
        }
    }

    #[test]
    fn bandpass_is_symmetric() {
        let h = design_bandpass(129, 0.05, 0.15);
        for k in 0..h.len() / 2 {
            let mirrored = h[h.len() - 1 - k];
            assert!((h[k] - mirrored).abs() < 1e-7, "asymmetric at tap {k}");
        }
    }
}

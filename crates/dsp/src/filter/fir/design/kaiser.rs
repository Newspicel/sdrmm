use super::{MIN_TAPS, response_db, window_sinc::windowed_sinc};
use crate::math::special::bessel_i0;

/// Kaiser's length estimate, `taps - 1 = (attenuation - OFFSET) / (SLOPE * transition)`.
const KAISER_OFFSET_DB: f64 = 7.95;
const KAISER_SLOPE: f64 = 14.36;
/// Kaiser's fit of the window shape to the attenuation: `STEEP` above `BETA_STEEP_ABOVE_DB`,
/// `MILD` from `BETA_MILD_FROM_DB`, a rectangle below.
const BETA_STEEP_ABOVE_DB: f64 = 50.0;
const BETA_STEEP: [f64; 2] = [0.1102, 8.7];
const BETA_MILD_FROM_DB: f64 = 21.0;
const BETA_MILD: [f64; 3] = [0.5842, 0.4, 0.07886];
/// A half-band filter cuts off at a quarter of the rate.
const HALFBAND_CUTOFF: f64 = 0.25;
/// The longest half-band the length search tries, as a multiple of the Kaiser estimate.
const HALFBAND_SEARCH_SPAN: usize = 4;
const STOPBAND_PROBES: usize = 512;

#[must_use]
pub fn kaiser_taps(transition: f64, attenuation_db: f64) -> usize {
    let wanted = ((attenuation_db - KAISER_OFFSET_DB) / (KAISER_SLOPE * transition)).ceil();
    let taps = (wanted.max(0.0) as usize).saturating_add(1) | 1;
    taps.max(MIN_TAPS)
}

#[must_use]
pub fn kaiser_transition(taps: usize, attenuation_db: f64) -> f64 {
    (attenuation_db - KAISER_OFFSET_DB) / (KAISER_SLOPE * taps.saturating_sub(1).max(1) as f64)
}

#[must_use]
pub fn kaiser_beta(attenuation_db: f64) -> f64 {
    if attenuation_db > BETA_STEEP_ABOVE_DB {
        BETA_STEEP[0] * (attenuation_db - BETA_STEEP[1])
    } else if attenuation_db >= BETA_MILD_FROM_DB {
        let excess = attenuation_db - BETA_MILD_FROM_DB;
        BETA_MILD[0] * excess.powf(BETA_MILD[1]) + BETA_MILD[2] * excess
    } else {
        0.0
    }
}

/// Windowed-sinc low-pass under a Kaiser window sized for `attenuation_db` of stopband.
#[must_use]
pub fn design_lowpass_kaiser(taps: usize, cutoff: f64, attenuation_db: f64) -> Vec<f32> {
    let center = taps.saturating_sub(1) as f64 / 2.0;
    let beta = kaiser_beta(attenuation_db);
    let scale = bessel_i0(beta);
    windowed_sinc(taps, cutoff, |k| {
        let x = (k as f64 - center) / center;
        bessel_i0(beta * (1.0 - x * x).max(0.0).sqrt()) / scale
    })
}

/// The shortest half-band length, `4k + 3`, whose measured stopband reaches `attenuation_db`
/// within `transition`. The Kaiser estimate falls a few dB short for short filters, so the
/// length grows from it until the response meets the attenuation.
#[must_use]
pub fn halfband_taps(transition: f64, attenuation_db: f64) -> usize {
    let estimate = kaiser_taps(transition, attenuation_db);
    let first = if estimate % 4 == 1 {
        estimate + 2
    } else {
        estimate
    };
    let last = first.saturating_mul(HALFBAND_SEARCH_SPAN);
    let stop = HALFBAND_CUTOFF + transition / 2.0;
    (first..=last)
        .step_by(4)
        .find(|&taps| {
            stopband_peak_db(&design_halfband(taps, attenuation_db), stop) <= -attenuation_db
        })
        .unwrap_or(last | 3)
}

/// A Kaiser half-band low-pass, cut off at a quarter of the rate. Every even offset from the
/// center is exactly zero, so a decimator by two skips half of the products.
///
/// # Panics
/// Panics unless `taps` is congruent to 3 modulo 4, the length that keeps the outermost taps
/// nonzero.
#[must_use]
pub fn design_halfband(taps: usize, attenuation_db: f64) -> Vec<f32> {
    assert!(taps % 4 == 3, "a half-band filter has 4k+3 taps");
    let center = taps / 2;
    let mut h = design_lowpass_kaiser(taps, HALFBAND_CUTOFF, attenuation_db);
    for (index, tap) in h.iter_mut().enumerate() {
        if index != center && index.abs_diff(center).is_multiple_of(2) {
            *tap = 0.0;
        }
    }
    h
}

fn stopband_peak_db(h: &[f32], stop: f64) -> f64 {
    (0..=STOPBAND_PROBES)
        .map(|step| {
            response_db(
                h,
                stop + (0.5 - stop) * step as f64 / STOPBAND_PROBES as f64,
            )
        })
        .fold(f64::NEG_INFINITY, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::db::Decibels;

    #[test]
    fn a_kaiser_design_reaches_its_stopband_within_the_predicted_transition() {
        for (taps, attenuation_db) in [(64, 72.0), (128, 72.0), (131, 100.0)] {
            let width = kaiser_transition(taps, attenuation_db);
            let h = design_lowpass_kaiser(taps, 0.2, attenuation_db);
            let stop = 0.2 + width / 2.0;
            let worst = (0..200)
                .map(|step| response_db(&h, stop + (0.5 - stop) * f64::from(step) / 200.0))
                .fold(f64::NEG_INFINITY, f64::max);
            assert!(worst < 3.0 - attenuation_db, "{taps} taps: {worst:.1} dB");
            assert!(kaiser_taps(width, attenuation_db) <= taps + 2);
        }
    }

    #[test]
    fn a_halfband_is_exactly_zero_at_even_offsets_and_meets_its_edges() {
        for (transition, attenuation_db) in [(0.4, 100.0), (0.2, 100.0), (0.05, 100.0), (0.1, 72.0)]
        {
            let taps = halfband_taps(transition, attenuation_db);
            assert_eq!(taps % 4, 3);
            let h = design_halfband(taps, attenuation_db);
            let center = taps / 2;
            for (index, &tap) in h.iter().enumerate() {
                let even = index.abs_diff(center) % 2 == 0;
                assert_eq!(
                    tap == 0.0,
                    even && index != center,
                    "{taps} taps, index {index}"
                );
            }
            let pass = 0.25 - transition / 2.0;
            let stop = 0.25 + transition / 2.0;
            for step in 0..=200 {
                let f = f64::from(step) / 200.0;
                let ripple = response_db(&h, pass * f).abs();
                let allowed = (1.0 + 2.0 * (-attenuation_db).db_to_amplitude()).amplitude_to_db();
                assert!(ripple < allowed, "{taps} taps: {ripple} dB ripple");
                let leak = response_db(&h, stop + (0.5 - stop) * f);
                assert!(leak < 3.0 - attenuation_db, "{taps} taps: {leak:.1} dB");
            }
        }
    }

    #[test]
    fn an_attenuation_below_the_window_floor_still_designs() {
        for attenuation_db in [0.0, 3.0, 7.0, f64::NAN] {
            let taps = kaiser_taps(0.1, attenuation_db);
            assert!(taps >= MIN_TAPS, "{attenuation_db} dB gave {taps} taps");
            assert_eq!(taps % 2, 1);
            let h = design_lowpass_kaiser(taps, 0.2, attenuation_db);
            assert_eq!(h.len(), taps);
        }
    }
}

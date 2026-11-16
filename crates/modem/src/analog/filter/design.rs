use std::f64::consts::PI;

use num_complex::Complex;
use sdrmm_dsp::math::complex::narrow;

use super::{MAX_TAPS, MIN_TAPS};

const BLACKMAN: [f64; 3] = [0.42, 0.5, 0.08];
/// Frequency grid points per tap when sampling a vestigial response.
const VESTIGIAL_OVERSAMPLE: usize = 16;

#[must_use]
pub fn design_from_response(
    taps: usize,
    oversample: usize,
    response: impl Fn(f64) -> Complex<f64>,
) -> Vec<Complex<f32>> {
    assert!(
        (MIN_TAPS..=MAX_TAPS).contains(&taps),
        "band filters run 3..={MAX_TAPS} taps, got {taps}"
    );
    let taps = taps | 1;
    let centre = (taps - 1) as f64 / 2.0;
    let grid = (taps * oversample.max(1)).next_power_of_two();
    let window = blackman(taps);
    (0..taps)
        .map(|k| {
            let offset = k as f64 - centre;
            let mut acc = Complex::new(0.0, 0.0);
            for g in 0..grid {
                let f = (g as f64 + 0.5) / grid as f64 - 0.5;
                acc += response(f) * Complex::from_polar(1.0, 2.0 * PI * f * offset);
            }
            narrow(acc / grid as f64 * window[k])
        })
        .collect()
}

fn blackman(taps: usize) -> Vec<f64> {
    let n = (taps - 1) as f64;
    (0..taps)
        .map(|k| {
            let x = 2.0 * PI * k as f64 / n;
            BLACKMAN[0] - BLACKMAN[1] * x.cos() + BLACKMAN[2] * (2.0 * x).cos()
        })
        .collect()
}

#[must_use]
pub fn design_hilbert(taps: usize) -> Vec<f32> {
    assert!(
        (MIN_TAPS..=MAX_TAPS).contains(&taps) && !taps.is_multiple_of(2),
        "a Hilbert transformer needs an odd 3..={MAX_TAPS} taps, got {taps}"
    );
    let centre = (taps - 1) as isize / 2;
    let window = blackman(taps);
    (0..taps)
        .map(|k| {
            let n = k as isize - centre;
            if n % 2 == 0 {
                0.0
            } else {
                (2.0 / (PI * n as f64) * window[k]) as f32
            }
        })
        .collect()
}

#[must_use]
pub fn design_vestigial(taps: usize, vestige: f64, upper: f64) -> Vec<Complex<f32>> {
    assert!(
        vestige > 0.0 && vestige < upper && upper < 0.5,
        "need 0 < vestige < upper < ½, got vestige {vestige}, upper {upper}"
    );
    design_from_response(taps, VESTIGIAL_OVERSAMPLE, |f| {
        let gain = if f < -vestige || f > upper {
            0.0
        } else if f <= vestige {
            f64::midpoint(1.0, (PI * f / (2.0 * vestige)).sin())
        } else {
            1.0
        };
        Complex::new(gain, 0.0)
    })
}

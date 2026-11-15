use std::f64::consts::TAU;

use num_complex::Complex;

use super::{equaliser_at, frequency::bin_frequency};
use crate::{
    math::complex::{narrow, widen},
    math::fft::FftPair,
    math::special::{bessel_i0, sinc},
};

/// Equaliser magnitude below which a bin is left undivided rather than blown up.
const EQ_FLOOR: f64 = 1e-6;

pub fn design_correction(
    fft_len: usize,
    taps: usize,
    beta: f32,
    delay_frac: f32,
    weight: Complex<f32>,
    equaliser: Option<&[Complex<f32>]>,
    out: &mut Vec<Complex<f32>>,
) {
    let len = fft_len.max(1);
    let taps = taps.clamp(1, len);
    let centre = (taps - 1) as f64 / 2.0 - f64::from(delay_frac);
    let window = kaiser(taps, centre, f64::from(beta));
    let mut response: Vec<Complex<f64>> = window
        .iter()
        .enumerate()
        .map(|(n, w)| Complex::new(w * sinc(n as f64 - centre), 0.0))
        .collect();
    let dc: Complex<f64> = response.iter().sum();
    if dc.norm() > f64::MIN_POSITIVE {
        for tap in &mut response {
            *tap /= dc;
        }
    }
    if let Some(equaliser) = equaliser.filter(|points| !points.is_empty()) {
        divide_by(&mut response, equaliser);
        for (tap, w) in response.iter_mut().zip(&window) {
            *tap *= w;
        }
    }
    let weight = widen(weight);
    out.clear();
    out.extend(response.iter().map(|tap| narrow(tap * weight)));
    out.resize(len, Complex::default());
    FftPair::new(len).forward(out);
}

fn kaiser(taps: usize, centre: f64, beta: f64) -> Vec<f64> {
    let half = centre.abs().max((taps as f64 - 1.0 - centre).abs());
    if half <= f64::MIN_POSITIVE {
        return vec![1.0; taps];
    }
    let scale = bessel_i0(beta);
    (0..taps)
        .map(|n| {
            let x = (n as f64 - centre) / half;
            bessel_i0(beta * (1.0 - x * x).max(0.0).sqrt()) / scale
        })
        .collect()
}

fn divide_by(response: &mut [Complex<f64>], equaliser: &[Complex<f32>]) {
    let taps = response.len();
    let spectrum: Vec<Complex<f64>> = (0..taps)
        .map(|bin| {
            let value: Complex<f64> = response
                .iter()
                .enumerate()
                .map(|(n, tap)| tap * twiddle(-((bin * n % taps) as f64), taps))
                .sum();
            let eq = widen(equaliser_at(equaliser, bin_frequency(bin, taps) as f32));
            if eq.norm() > EQ_FLOOR {
                value / eq
            } else {
                value
            }
        })
        .collect();
    for (n, tap) in response.iter_mut().enumerate() {
        *tap = spectrum
            .iter()
            .enumerate()
            .map(|(bin, value)| value * twiddle((bin * n % taps) as f64, taps))
            .sum::<Complex<f64>>()
            / taps as f64;
    }
}

fn twiddle(turns: f64, len: usize) -> Complex<f64> {
    Complex::from_polar(1.0, TAU * turns / len as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        array::array_sync::{
            FastConvolver,
            signals::{Gaussian, delay_between},
        },
        math::db::Decibels,
        math::special::wrap_rad,
    };

    const FFT: usize = 4096;
    const TAPS: usize = 129;
    const BETA: f32 = 8.0;

    fn design(delay: f32, weight: Complex<f32>, eq: Option<&[Complex<f32>]>) -> Vec<Complex<f32>> {
        let mut out = Vec::new();
        design_correction(FFT, TAPS, BETA, delay, weight, eq, &mut out);
        out
    }

    #[test]
    fn correction_delays_by_the_designed_fraction() {
        let mut convolver = FastConvolver::new(FFT, TAPS);
        convolver
            .set_response(&design(0.3, Complex::new(1.0, 0.0), None))
            .unwrap();
        let input = Gaussian::new(21).block(40 * convolver.hop());
        let mut output = Vec::new();
        convolver.push(&input, &mut output);
        let whole = (TAPS - 1) / 2;
        let measured = delay_between(&input[..output.len() - whole], &output[whole..], 1024);
        assert!((measured + 0.3).abs() < 0.01, "{measured}");
    }

    #[test]
    fn correction_is_flat_to_0_45_fs() {
        for delay in [0.0f32, 0.3, -0.45, 0.5] {
            let weight = Complex::from_polar(0.7, 1.1);
            let spectrum = design(delay, weight, None);
            let group = (TAPS - 1) as f64 / 2.0 - f64::from(delay);
            for (bin, value) in spectrum.iter().enumerate() {
                let nu = bin_frequency(bin, FFT);
                if nu.abs() > 0.45 {
                    continue;
                }
                let gain_db = (f64::from(value.norm()) / 0.7).amplitude_to_db();
                assert!(gain_db.abs() < 0.1, "delay {delay} nu {nu}: {gain_db} dB");
                let expected = 1.1 - TAU * nu * group;
                let error = wrap_rad(f64::from(value.arg()) - expected).to_degrees();
                assert!(error.abs() < 0.5, "delay {delay} nu {nu}: {error} deg");
            }
        }
    }

    #[test]
    fn an_equaliser_is_divided_out_of_the_correction() {
        let ripple_db = |nu: f64| (TAU * 3.0 * nu).cos();
        let points = 64;
        let equaliser: Vec<Complex<f32>> = (0..points)
            .map(|point| {
                let nu = -0.5 + point as f64 / points as f64;
                Complex::from_polar(ripple_db(nu).db_to_amplitude() as f32, 0.0)
            })
            .collect();
        let spectrum = design(0.2, Complex::new(1.0, 0.0), Some(&equaliser));
        let group = (TAPS - 1) as f64 / 2.0 - 0.2;
        for (bin, value) in spectrum.iter().enumerate() {
            let nu = bin_frequency(bin, FFT);
            if nu.abs() > 0.4 {
                continue;
            }
            let gain_db = f64::from(value.norm()).amplitude_to_db();
            assert!(
                (gain_db + ripple_db(nu)).abs() < 0.1,
                "nu {nu}: {gain_db} dB against {}",
                -ripple_db(nu)
            );
            let error = wrap_rad(f64::from(value.arg()) + TAU * nu * group).to_degrees();
            assert!(error.abs() < 0.5, "nu {nu}: {error} deg");
        }
    }

    #[test]
    fn one_tap_is_a_plain_weight() {
        let mut out = Vec::new();
        design_correction(8, 1, BETA, 0.0, Complex::new(0.0, 2.0), None, &mut out);
        assert_eq!(out.len(), 8);
        assert!(
            out.iter()
                .all(|value| (value - Complex::new(0.0, 2.0)).norm() < 1e-6)
        );
    }
}

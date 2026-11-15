use std::f64::consts::PI;

use num_complex::Complex;
use sdrmm_dsp::{design_lowpass, math::complex::widen};

use super::*;

fn response(taps: &[Complex<f32>], f: f64) -> Complex<f64> {
    taps.iter()
        .enumerate()
        .map(|(k, &t)| widen(t) * Complex::from_polar(1.0, -2.0 * PI * f * k as f64))
        .sum()
}

fn real_response(taps: &[f32], f: f64) -> Complex<f64> {
    let complex: Vec<Complex<f32>> = taps.iter().map(|&t| Complex::new(t, 0.0)).collect();
    response(&complex, f)
}

#[test]
fn a_symmetric_band_is_a_real_filter_and_an_offset_one_is_not() {
    assert!(matches!(
        BandFilter::symmetric(0.1, 65).build(),
        Band::Real(_)
    ));
    assert!(matches!(
        BandFilter {
            low: 0.01,
            high: 0.1,
            taps: 65
        }
        .build(),
        Band::Complex(_)
    ));
}

#[test]
fn a_band_at_the_nyquist_edge_still_builds() {
    for (low, high) in [(0.4999, 0.5), (0.6, 0.7), (-0.7, -0.6), (0.5, 0.4)] {
        let mut band = BandFilter {
            low,
            high,
            taps: 33,
        }
        .build();
        let mut out = Vec::new();
        band.process(&[Complex::new(1.0, 0.0); 64], &mut out);
        assert_eq!(out.len(), 64, "band {low}..{high} produced no output");
        assert!(
            out.iter().all(|s| s.re.is_finite() && s.im.is_finite()),
            "band {low}..{high} produced a non-finite sample"
        );
    }
}

#[test]
fn an_offset_band_passes_its_own_side_only() {
    let mut band = BandFilter {
        low: 0.02,
        high: 0.12,
        taps: 129,
    }
    .build();
    let level = |f: f64| {
        let x: Vec<Complex<f32>> = (0..4096)
            .map(|n| Complex::from_polar(1.0, (2.0 * PI * f * n as f64) as f32))
            .collect();
        let mut out = Vec::new();
        let mut band = BandFilter {
            low: 0.02,
            high: 0.12,
            taps: 129,
        }
        .build();
        band.process(&x, &mut out);
        let tail = &out[512..];
        (tail.iter().map(|s| f64::from(s.norm_sqr())).sum::<f64>() / tail.len() as f64).sqrt()
    };
    let mut sink = Vec::new();
    band.process(&[Complex::new(0.0, 0.0); 8], &mut sink);
    assert!((level(0.07) - 1.0).abs() < 0.02, "passband {}", level(0.07));
    assert!(level(-0.07) < 0.01, "image {}", level(-0.07));
    assert!(level(0.25) < 0.01, "stopband {}", level(0.25));
}

#[test]
fn the_hilbert_response_is_a_quarter_turn_each_way() {
    let taps = design_hilbert(129);
    for &f in &[0.05, 0.1, 0.2, 0.4] {
        let h = real_response(&taps, f);
        assert!(
            (h.norm() - 1.0).abs() < 0.02,
            "magnitude at {f}: {}",
            h.norm()
        );
        let delay = Complex::from_polar(1.0, 2.0 * PI * f * ((taps.len() - 1) / 2) as f64);
        let turn = (h * delay).arg();
        assert!(
            (turn + std::f64::consts::FRAC_PI_2).abs() < 0.02,
            "phase at {f}: {turn}"
        );
        let mirror = real_response(&taps, -f) * delay.conj();
        assert!(
            (mirror.arg() - std::f64::consts::FRAC_PI_2).abs() < 0.02,
            "phase at -{f}: {}",
            mirror.arg()
        );
    }
    for (k, &t) in taps.iter().enumerate() {
        if (k as isize - 64) % 2 == 0 {
            assert_eq!(t, 0.0, "tap {k}");
        }
    }
}

#[test]
fn the_vestigial_skirts_add_to_a_constant() {
    let taps = design_vestigial(257, 0.01, 0.1);
    let delay = |f: f64| Complex::from_polar(1.0, 2.0 * PI * f * 128.0);
    let at = |f: f64| (response(&taps, f) * delay(f)).re;
    assert!((at(0.0) - 0.5).abs() < 0.01, "carrier {}", at(0.0));
    for &f in &[0.002, 0.004, 0.006, 0.008] {
        let sum = at(f) + at(-f);
        assert!((sum - 1.0).abs() < 0.02, "skirts at ±{f} sum to {sum}");
    }
    assert!((at(0.05) - 1.0).abs() < 0.02, "upper sideband {}", at(0.05));
    assert!(at(-0.05).abs() < 0.02, "carved sideband {}", at(-0.05));
    assert!(at(0.2).abs() < 0.02, "stopband {}", at(0.2));
}

#[test]
fn the_designer_reproduces_a_plain_lowpass() {
    let designed = design_from_response(65, 16, |f| {
        Complex::new(f64::from(u8::from(f.abs() <= 0.1)), 0.0)
    });
    let reference = design_lowpass(65, 0.1);
    for &f in &[0.0, 0.05, 0.09, 0.2, 0.4] {
        let a = response(&designed, f).norm();
        let b = real_response(&reference, f).norm();
        assert!((a - b).abs() < 0.03, "at {f}: designed {a}, reference {b}");
    }
}

use sdrmm_dsp::Decibels;

use super::{demod::argmax_bin, *};
use crate::testutil::{add_noise, draws};

const TIMING_SEEDS: u64 = 20;

/// The widest miss allowed at 13 dB. A whole-sample slip moves the carrier by whole bins, which
/// turns no phase between symbols, so only the steps between known symbols reveal it. Seed 4
/// draws `[49, 52, 0, 4, 4, 88, 84, 97]`: five of its seven steps lie within 13 of zero, so a
/// lead two samples off keeps 0.73 of the true lead's coherence.
const AMBIGUOUS_SLIP: usize = 2;

/// Symbols whose early window does not cross the chirp's wrap, so the peak keeps 9/16 of its energy.
const SPLIT_SYMBOLS: [u32; 8] = [112, 60, 98, 7, 126, 77, 117, 103];

#[test]
fn distinct_shifts_are_orthogonal_at_every_spreading_factor() {
    for sf in MIN_SPREADING_FACTOR..=10 {
        let params = CssParams::new(sf);
        let n = params.chips();
        let modulator = CssMod::new(params);
        let symbol_of = |s: u32| {
            let mut out = Vec::new();
            modulator.modulate(&[s], &mut out);
            out
        };
        let a = symbol_of(0);
        let energy: f64 = a.iter().map(|c| f64::from(c.norm_sqr())).sum();
        assert!((energy - 1.0).abs() < 1e-5, "SF{sf} energy {energy}");
        for shift in [1u32, 3, (n / 2) as u32, (n - 1) as u32] {
            let b = symbol_of(shift);
            let inner: Complex<f64> = a
                .iter()
                .zip(&b)
                .map(|(&x, &y)| {
                    Complex::new(f64::from(x.re), f64::from(x.im))
                        * Complex::new(f64::from(y.re), -f64::from(y.im))
                })
                .sum();
            assert!(
                inner.norm() < 1e-4,
                "SF{sf} shift {shift}: inner product {inner}"
            );
        }
    }
}

#[test]
fn every_spreading_factor_round_trips_and_stays_constant_envelope() {
    for sf in MIN_SPREADING_FACTOR..=MAX_SPREADING_FACTOR {
        let params = CssParams::new(sf);
        let n = params.chips();
        let symbols = draws(24, n, 0xc55 + u64::from(sf));
        let mut wave = Vec::new();
        CssMod::new(params.clone()).modulate(&symbols, &mut wave);
        let amplitude = (n as f32).sqrt().recip();
        for (k, s) in wave.iter().enumerate() {
            assert!(
                (s.norm() - amplitude).abs() < 1e-5,
                "SF{sf} sample {k} modulus {}",
                s.norm()
            );
        }
        let mut got = Vec::new();
        CssDemod::new(params).demodulate(&wave, 0, symbols.len(), &mut got);
        assert_eq!(got, symbols, "SF{sf}");
    }
}

fn timed_preamble(params: &CssParams, preamble: &[u32], lead: usize) -> Vec<Complex<f32>> {
    let n = params.chips();
    let mut wave = vec![Complex::new(0.0, 0.0); lead];
    CssMod::new(params.clone()).modulate(preamble, &mut wave);
    wave.resize(wave.len() + 2 * n, Complex::new(0.0, 0.0));
    wave
}

/// Every lead below half a symbol, over many preambles: `(found, leads, worst miss)`.
fn timing_sweep(noise_var: f64) -> (usize, usize, usize) {
    let params = CssParams::new(7);
    let n = params.chips();
    let (mut found, mut leads, mut worst) = (0, 0, 0);
    for seed in 0..TIMING_SEEDS {
        let preamble: Vec<u32> = draws(8, n, seed);
        let mut demod = CssDemod::new(params.clone());
        for lead in 0..n / 2 {
            let mut wave = timed_preamble(&params, &preamble, lead);
            add_noise(&mut wave, 0x7158 + seed * n as u64 + lead as u64, noise_var);
            let origin = demod.estimate_origin(&wave, &preamble);
            leads += 1;
            if origin == lead {
                found += 1;
            }
            worst = worst.max(origin.abs_diff(lead));
        }
    }
    (found, leads, worst)
}

#[test]
fn the_timing_estimate_finds_every_lead_below_half_a_symbol() {
    let (found, leads, worst) = timing_sweep(0.02);
    assert_eq!(found, leads, "worst miss {worst} samples");
}

#[test]
fn at_13_db_only_slips_the_preamble_cannot_resolve_are_missed() {
    let (found, leads, worst) = timing_sweep(0.05);
    assert!(found * 200 >= leads * 199, "found {found} of {leads}");
    assert!(worst <= AMBIGUOUS_SLIP, "missed by {worst} samples");
}

#[test]
fn a_lead_past_half_a_symbol_is_not_taken_for_itself() {
    let params = CssParams::new(7);
    let n = params.chips();
    for seed in 0..TIMING_SEEDS {
        let preamble: Vec<u32> = draws(8, n, seed);
        let wave = timed_preamble(&params, &preamble, 100);
        let origin = CssDemod::new(params.clone()).estimate_origin(&wave, &preamble);
        assert_ne!(origin, 100, "seed {seed}");
    }
}

#[test]
fn a_timing_error_moves_a_chirps_peak_rather_than_shrinking_it() {
    let params = CssParams::new(7);
    let n = params.chips();
    let symbols = SPLIT_SYMBOLS;
    let lead = 32usize;
    let mut wave = vec![Complex::new(0.0, 0.0); lead];
    CssMod::new(params.clone()).modulate(&symbols, &mut wave);
    wave.resize(wave.len() + 256, Complex::new(0.0, 0.0));
    let mut demod = CssDemod::new(params);
    let mut energies = vec![0.0f32; n];
    let peak_at = |demod: &mut CssDemod, origin: usize, energies: &mut Vec<f32>| {
        demod.energies(&wave, origin, 1, energies);
        energies.iter().copied().fold(0.0f32, f32::max)
    };
    let aligned = peak_at(&mut demod, lead, &mut energies);
    let misaligned = peak_at(&mut demod, 0, &mut energies);
    assert!(
        (misaligned / aligned - 0.5625).abs() < 0.05,
        "aligned {aligned}, 32 samples early {misaligned}"
    );
    demod.energies(&wave, 0, 1, &mut energies);
    let moved = argmax_bin(&energies);
    demod.energies(&wave, lead, 1, &mut energies);
    let correct = argmax_bin(&energies);
    assert_eq!(correct, symbols[1]);
    assert_eq!(moved, (symbols[1] + n as u32 - lead as u32) % n as u32);
}

#[test]
fn sensitivity_improves_with_the_spreading_factor() {
    let mut previous = usize::MAX;
    for sf in [7u32, 8, 9, 10] {
        let params = CssParams::new(sf);
        let n = params.chips();
        let count = 4_000 / sf as usize;
        let symbols = draws(count, n, 0x5e05 + u64::from(sf));
        let mut wave = Vec::new();
        CssMod::new(params.clone()).modulate(&symbols, &mut wave);
        add_noise(&mut wave, 0x5e06 + u64::from(sf), 0.45);
        let mut got = Vec::new();
        CssDemod::new(params).demodulate(&wave, 0, symbols.len(), &mut got);
        let errors = got.iter().zip(&symbols).filter(|(a, b)| a != b).count();
        assert!(errors > 0, "SF{sf} too clean to order");
        assert!(
            errors < previous,
            "SF{sf}: {errors} symbol errors, SF{} had {previous}",
            sf - 1
        );
        previous = errors;
    }
}

fn offset(wave: &mut [Complex<f32>], bins_per_symbol: f64, drift: f64, n: usize) {
    let mut phase = 0.0f64;
    for (k, s) in wave.iter_mut().enumerate() {
        let symbol = k as f64 / n as f64;
        let bins = bins_per_symbol + drift * symbol;
        phase += std::f64::consts::TAU * bins / n as f64;
        *s *= Complex::new(phase.cos() as f32, phase.sin() as f32);
    }
}

#[test]
fn a_fractional_offset_and_its_drift_are_tracked() {
    let params = CssParams::new(9);
    let n = params.chips();
    let preamble = draws(16, n, 0x0f0f);
    let symbols = draws(400, n, 0x1f1f);
    let mut wave = Vec::new();
    CssMod::new(params.clone()).frame(&preamble, &symbols, &mut wave);
    offset(&mut wave, 0.37, 0.004, n);
    add_noise(&mut wave, 0x2f2f, 0.02);
    let mut demod = CssDemod::new(params);
    let origin = demod.estimate_origin(&wave, &preamble);
    assert_eq!(origin, 0);
    let mut got = Vec::new();
    demod.demodulate(&wave, preamble.len() * n, symbols.len(), &mut got);
    let errors = got.iter().zip(&symbols).filter(|(a, b)| a != b).count();
    assert_eq!(errors, 0, "offset read {}", demod.offset_bins());
    assert!((demod.offset_bins() - 2.03).abs() < 0.05);
}

#[test]
fn the_noise_estimate_recovers_a_known_n0() {
    let params = CssParams::new(7);
    let n = params.chips();
    let symbols = draws(300, n, 0x0e51);
    let mut wave = Vec::new();
    CssMod::new(params.clone()).modulate(&symbols, &mut wave);
    add_noise(&mut wave, 0x0e52, 0.5);
    let estimate = CssDemod::new(params).noise_var(&wave, 0, symbols.len());
    assert!((estimate - 0.5).abs() < 0.02, "estimated N0 {estimate}");
}

#[test]
fn llr_magnitudes_predict_their_own_error_rate() {
    let params = CssParams::new(6);
    let n = params.chips();
    let symbols = draws(6_000, n, 0x11c6);
    let mut wave = Vec::new();
    CssMod::new(params.clone()).modulate(&symbols, &mut wave);
    add_noise(&mut wave, 0x11c7, 0.125);
    let mut demod = CssDemod::new(params.clone());
    let mut llrs = Vec::new();
    demod.llrs(&wave, 0, symbols.len(), 0.125, &mut llrs);
    let bits = params.bits_per_symbol();
    let mut bands = [(0u32, 0u32); 3];
    for (i, &llr) in llrs.iter().enumerate() {
        let sent = (symbols[i / bits] >> (i % bits)) & 1 == 1;
        let band = match llr.0.abs() {
            x if x < 1.0 => 0,
            x if x < 3.0 => 1,
            _ => 2,
        };
        bands[band].0 += 1;
        if llr.bit() != sent {
            bands[band].1 += 1;
        }
    }
    for (band, &(count, wrong)) in bands.iter().enumerate() {
        assert!(count > 300, "band {band} saw only {count} bits");
        let measured = f64::from(wrong) / f64::from(count);
        let predicted = 1.0 / (1.0 + [0.5f64, 2.0, 4.0][band].exp());
        assert!(
            measured < predicted * 3.0 + 0.02,
            "band {band}: measured {measured}, predicted {predicted}"
        );
    }
}

#[test]
fn the_framing_overhead_is_the_symbol_count_ratio() {
    assert!(CssParams::framing_overhead_db(0, 64).abs() < 1e-12);
    let overhead = CssParams::framing_overhead_db(8, 256);
    assert!((overhead - (264.0f64 / 256.0).power_to_db()).abs() < 1e-12);
    assert!((overhead - 0.1336).abs() < 1e-3, "{overhead}");
}

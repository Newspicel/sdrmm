use std::f32::consts::FRAC_1_SQRT_2;

use num_complex::Complex;
use sdrmm_modem_test_support::ber::rng::Rng;

use super::{super::chip::ChipShaper, *};
use crate::testutil::draws;

const SPS: usize = 4;
const LEAD: usize = 41;
const PREAMBLE: usize = 32;

fn params(mode: CckMode) -> CckParams {
    CckParams::new(mode, ChipShaper::root_raised_cosine(SPS, 0.35, 8), 8)
}

fn labels(mode: CckMode, count: usize, seed: u64) -> Vec<u32> {
    draws(count, mode.alphabet() as usize, seed)
}

fn transmit(p: &CckParams, preamble: &[u32], payload: &[u32]) -> Vec<Complex<f32>> {
    let mut wave = vec![Complex::new(0.0, 0.0); LEAD];
    CckMod::new(p.clone()).frame(preamble, payload, &mut wave);
    wave.resize(wave.len() + 128, Complex::new(0.0, 0.0));
    wave
}

#[test]
fn the_generated_codebook_is_constant_modulus_distinct_and_factors_phi1_out() {
    for mode in [CckMode::Bits4, CckMode::Bits8] {
        let book = Codebook::new(mode);
        assert_eq!(book.words().len(), mode.words());
        for (i, word) in book.words().iter().enumerate() {
            for (c, chip) in word.iter().enumerate() {
                assert!(
                    (chip.norm() - 1.0).abs() < 1e-6,
                    "{mode:?} word {i} chip {c} modulus {}",
                    chip.norm()
                );
            }
            for (j, other) in book.words().iter().enumerate().skip(i + 1) {
                let same = word.iter().zip(other).all(|(a, b)| (a - b).norm() < 1e-6);
                assert!(!same, "{mode:?} words {i} and {j} coincide");
            }
        }
        let mut a = [Complex::new(0.0, 0.0); CHIPS];
        let mut b = [Complex::new(0.0, 0.0); CHIPS];
        let base = 2u32 << 2;
        book.chips(base, 0, &mut a);
        book.chips(base + 1, 0, &mut b);
        let ratio = b[0] / a[0];
        for c in 1..CHIPS {
            assert!((b[c] / a[c] - ratio).norm() < 1e-5, "{mode:?} chip {c}");
        }
        assert!((ratio - Complex::new(0.0, 1.0)).norm() < 1e-5);
    }
}

#[test]
fn the_codebook_is_a_block_code_with_the_distance_that_earns_its_rate() {
    let book = Codebook::new(CckMode::Bits8);
    let chips_of = |label: u32| {
        let mut out = [Complex::new(0.0, 0.0); CHIPS];
        book.chips(label, 0, &mut out);
        out
    };
    let d2 = |x: u32, y: u32| -> f64 {
        chips_of(x)
            .iter()
            .zip(&chips_of(y))
            .map(|(&p, &q)| f64::from((p - q).norm_sqr()))
            .sum()
    };
    let mut d2_min = f64::INFINITY;
    for x in 0..256u32 {
        for y in 0..256u32 {
            if x != y {
                d2_min = d2_min.min(d2(x, y));
            }
        }
    }
    assert!((d2_min - 1.0).abs() < 1e-5, "d²_min {d2_min}");

    for x in 0..256u32 {
        let neighbours = (0..256u32)
            .filter(|&y| y != x && (d2(x, y) - d2_min).abs() < 1e-5)
            .count();
        assert_eq!(
            neighbours, 24,
            "word {x} has {neighbours} nearest neighbours"
        );
    }

    assert!(
        (d2(0, 1) - 2.0).abs() < 1e-5,
        "φ1 quarter turn d² {}",
        d2(0, 1)
    );
}

#[test]
fn a_codeword_carries_unit_energy() {
    let book = Codebook::new(CckMode::Bits8);
    let mut word = [Complex::new(0.0, 0.0); CHIPS];
    for label in [0u32, 1, 37, 200, 255] {
        book.chips(label, 0, &mut word);
        let energy: f64 = word.iter().map(|c| f64::from(c.norm_sqr())).sum();
        assert!((energy - 1.0).abs() < 1e-6, "label {label} energy {energy}");
        assert!((word[0].norm() - CHIP_AMPLITUDE).abs() < 1e-6);
    }
}

#[test]
fn the_bank_recovers_a_clean_codeword_and_its_phase() {
    for mode in [CckMode::Bits4, CckMode::Bits8] {
        let book = Codebook::new(mode);
        let mut word = [Complex::new(0.0, 0.0); CHIPS];
        let mut bank = vec![Complex::new(0.0, 0.0); book.words().len()];
        for label in 0..mode.alphabet() {
            for symbol in 0..2 {
                book.chips(label, symbol, &mut word);
                book.correlate(&word, &mut bank);
                assert_eq!(book.decide(&bank, symbol), label, "{mode:?} label {label}");
                let winner = bank[(label >> 2) as usize % bank.len()];
                let want = book.reference(label, symbol);
                assert!((winner - want).norm() < 1e-5, "{mode:?} label {label}");
            }
        }
    }
}

#[test]
fn the_fast_bank_equals_the_direct_correlation() {
    for mode in [CckMode::Bits4, CckMode::Bits8] {
        let book = Codebook::new(mode);
        let chips: [Complex<f32>; CHIPS] = std::array::from_fn(|k| {
            Complex::new((k as f32 * 1.7).sin(), (k as f32 * 0.9 + 0.3).cos())
        });
        let mut bank = vec![Complex::new(0.0, 0.0); book.words().len()];
        book.correlate(&chips, &mut bank);
        for (index, word) in book.words().iter().enumerate() {
            let direct = chips
                .iter()
                .zip(word)
                .map(|(&y, &w)| y * w.conj())
                .sum::<Complex<f32>>()
                / (CHIPS as f32).sqrt();
            assert!(
                (bank[index] - direct).norm() < 1e-5,
                "{mode:?} word {index}: {} vs {direct}",
                bank[index]
            );
        }
    }
}

#[test]
fn both_rates_round_trip_and_find_their_own_origin() {
    for mode in [CckMode::Bits4, CckMode::Bits8] {
        let p = params(mode);
        let preamble = labels(mode, PREAMBLE, 0x0cc4);
        let payload = labels(mode, 400, 0x0cc8);
        let wave = transmit(&p, &preamble, &payload);
        let mut demod = CckDemod::new(p);
        let acquisition = demod
            .acquire(&wave, &preamble, LEAD + 48)
            .unwrap_or_else(|| panic!("{mode:?}: no acquisition"));
        assert_eq!(acquisition.origin, LEAD, "{mode:?}");
        let mut got = Vec::new();
        demod.demodulate(PREAMBLE, payload.len(), &mut got);
        assert_eq!(got, payload, "{mode:?}");
    }
}

#[test]
fn the_decision_is_joint_over_word_and_phase() {
    let book = Codebook::new(CckMode::Bits8);
    let mut bank = vec![Complex::new(0.0, 0.0); book.words().len()];
    bank[3] = Complex::new(FRAC_1_SQRT_2, FRAC_1_SQRT_2);
    bank[5] = Complex::new(0.95, 0.0);
    assert!(bank[3].norm() > bank[5].norm());
    let decided = book.decide(&bank, 0);
    assert_eq!(decided >> 2, 5, "decided word {}", decided >> 2);
    assert_eq!(decided & 3, 0);
}

#[test]
fn llr_magnitudes_predict_their_own_error_rate() {
    let mode = CckMode::Bits8;
    let p = params(mode);
    let preamble = labels(mode, 128, 0x11c4);
    let payload = labels(mode, 12_000, 0x11c5);
    let mut wave = transmit(&p, &preamble, &payload);
    let mut rng = Rng::new(0x11c6);
    let sigma = (0.08f64 / 2.0).sqrt();
    for s in wave.iter_mut() {
        *s += Complex::new((rng.normal() * sigma) as f32, (rng.normal() * sigma) as f32);
    }
    let mut demod = CckDemod::new(p);
    demod.acquire(&wave, &preamble, LEAD + 48).unwrap();
    let mut llrs = Vec::new();
    demod.llrs(preamble.len(), payload.len(), &mut llrs);

    let bits = mode.bits_per_symbol();
    let mut bands = [(0u32, 0u32); 3];
    for (i, &llr) in llrs.iter().enumerate() {
        let sent = (payload[i / bits] >> (i % bits)) & 1 == 1;
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
fn a_demodulator_with_no_acquisition_produces_nothing() {
    let demod = CckDemod::new(params(CckMode::Bits8));
    let mut labels = Vec::new();
    let mut llrs = Vec::new();
    demod.demodulate(PREAMBLE, 50, &mut labels);
    demod.llrs(PREAMBLE, 50, &mut llrs);
    assert!(labels.is_empty() && llrs.is_empty());
}

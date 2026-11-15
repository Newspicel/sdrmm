use std::f64::consts::PI;

use super::*;
use crate::{math::special::sinc, testutil::XorShift32};

const ROLLOFF: f64 = 0.35;
const SPAN: f64 = 6.0;

fn raised_cosine(t: f64) -> f64 {
    let denom = 1.0 - (2.0 * ROLLOFF * t).powi(2);
    if denom.abs() < 1e-8 {
        return std::f64::consts::FRAC_PI_4 * sinc(t);
    }
    sinc(t) * (PI * ROLLOFF * t).cos() / denom
}

fn symbols(count: usize, seed: u32) -> Vec<f32> {
    let mut rng = XorShift32(seed);
    (0..count)
        .map(|_| if rng.next_f32() >= 0.0 { 1.0 } else { -1.0 })
        .collect()
}

fn bpsk(syms: &[f32], sps: f64, offset: f64) -> Vec<Complex<f32>> {
    let len = ((syms.len() as f64 - SPAN) * sps) as usize;
    (0..len)
        .map(|n| {
            let t = n as f64 / sps - offset;
            let lo = (t - SPAN).ceil().max(0.0) as usize;
            let hi = ((t + SPAN) as usize).min(syms.len() - 1);
            let v: f64 = (lo..=hi)
                .map(|k| f64::from(syms[k]) * raised_cosine(t - k as f64))
                .sum();
            Complex::new(v as f32, 0.0)
        })
        .collect()
}

fn matches_at(syms: &[f32], out: &[Complex<f32>], settle: usize, d: usize) -> bool {
    let end = out.len().saturating_sub(4);
    settle < end
        && (settle..end).all(|j| j + d < syms.len() && (out[j].re > 0.0) == (syms[j + d] > 0.0))
}

fn alignment(syms: &[f32], out: &[Complex<f32>], settle: usize) -> Option<usize> {
    (0..8).find(|&d| matches_at(syms, out, settle, d))
}

#[test]
fn acquisition_starts_on_the_symbol_centres_at_any_offset() {
    let syms = symbols(80, 0x1234_5678);
    for eighth in 0..8 {
        let signal = bpsk(&syms, 4.0, f64::from(eighth) / 8.0);
        let mut sync = SymbolSync::new(4.0, 0.02).with_acquisition(32);
        let mut out = Vec::new();
        sync.process(&signal, &mut out);
        assert!(
            alignment(&syms, &out, 8).is_some(),
            "offset {eighth}/8: no alignment from the eighth symbol on"
        );
    }
}

#[test]
fn a_new_burst_starts_from_the_long_run_rate() {
    let syms = symbols(80, 0x1234_5678);
    let signal = bpsk(&syms, 4.0, 0.25);
    let mut sync = SymbolSync::new(4.0, 0.02).with_acquisition(32);
    sync.sps = 4.0 * (1.0 + TRACKING_RANGE);
    sync.process_held(&[], &mut Vec::new());
    let mut out = Vec::new();
    sync.process(&signal[..160], &mut out);
    assert!(
        (sync.sps() - 4.0).abs() < 0.05,
        "the last burst's drift carried over: {}",
        sync.sps()
    );
}

#[test]
fn acquisition_ignores_a_carrier_offset_under_the_symbols() {
    let syms = symbols(80, 0x1234_5678);
    for eighth in 0..8 {
        let mut signal = bpsk(&syms, 4.0, f64::from(eighth) / 8.0);
        for sample in &mut signal {
            *sample += Complex::new(0.7, 0.0);
        }
        let mut sync = SymbolSync::new(4.0, 0.02).with_acquisition(32);
        let mut out = Vec::new();
        sync.process(&signal, &mut out);
        for symbol in &mut out {
            *symbol -= Complex::new(0.7, 0.0);
        }
        assert!(
            alignment(&syms, &out, 8).is_some(),
            "offset {eighth}/8: no alignment from the eighth symbol on"
        );
    }
}

#[test]
fn recovers_bpsk_symbols_through_a_fractional_timing_offset() {
    let syms = symbols(600, 0x1234_5678);
    let signal = bpsk(&syms, 4.0, 0.37);
    let mut sync = SymbolSync::new(4.0, 0.02);
    let mut out = Vec::new();
    sync.process(&signal, &mut out);
    assert!(
        alignment(&syms, &out, 60).is_some(),
        "no offset aligns {} recovered symbols",
        out.len()
    );
}

fn four_level(count: usize, seed: u32) -> Vec<f32> {
    let mut rng = XorShift32(seed);
    (0..count)
        .map(|_| {
            let u = rng.next_f32();
            if u < -0.5 {
                -3.0
            } else if u < 0.0 {
                -1.0
            } else if u < 0.5 {
                1.0
            } else {
                3.0
            }
        })
        .collect()
}

fn residual(syms: &[f32], out: &[Complex<f32>], settle: usize) -> f32 {
    (0..8)
        .map(|d| {
            let n = out.len().saturating_sub(settle + 8);
            (settle..settle + n)
                .map(|j| (out[j].re - syms[j + d]).powi(2))
                .sum::<f32>()
                / n as f32
        })
        .fold(f32::INFINITY, f32::min)
        .sqrt()
}

#[test]
fn decisions_lock_four_levels_tighter_than_the_gardner_detector() {
    let syms = four_level(4_000, 0x4c4c_0001);
    for offset in [0.0, 0.3, 0.61] {
        let signal = bpsk(&syms, 8.0, offset);
        let run = |sync: SymbolSync| {
            let mut sync = sync;
            let mut out = Vec::new();
            sync.process(&signal, &mut out);
            residual(&syms, &out, 1_000)
        };
        let gardner = run(SymbolSync::new(8.0, 0.01));
        let decided = run(SymbolSync::new(8.0, 0.01).with_levels(&[-3.0, -1.0, 1.0, 3.0]));
        assert!(decided < 0.02, "offset {offset}: residual {decided}");
        assert!(
            decided < gardner,
            "offset {offset}: {decided} vs gardner {gardner}"
        );
    }
}

#[test]
fn a_long_run_of_one_inner_level_keeps_the_decisions_scaled() {
    let mut syms = four_level(1_500, 0x4c4c_0002);
    syms.extend(std::iter::repeat_n(1.0, 150));
    syms.extend(four_level(1_500, 0x4c4c_0003));
    let signal = bpsk(&syms, 8.0, 0.4);
    let mut sync = SymbolSync::new(8.0, 0.01).with_levels(&[-3.0, -1.0, 1.0, 3.0]);
    let mut out = Vec::new();
    sync.process(&signal, &mut out);
    let tail = residual(&syms, &out, 1_700);
    assert!(tail < 0.02, "residual after the run {tail}");
}

#[test]
fn ragged_blocks_match_one_shot_exactly() {
    let syms = symbols(400, 0x2bad_c0de);
    let signal = bpsk(&syms, 4.0, 0.21);

    let mut whole = SymbolSync::new(4.0, 0.02);
    let mut expected = Vec::new();
    whole.process(&signal, &mut expected);

    let mut ragged = SymbolSync::new(4.0, 0.02);
    let mut got = Vec::new();
    let mut pos = 0;
    for len in [1usize, 7, 64, 3, 129, 1024, 17].iter().cycle() {
        if pos >= signal.len() {
            break;
        }
        let end = (pos + len).min(signal.len());
        ragged.process(&signal[pos..end], &mut got);
        pos = end;
    }
    assert_eq!(expected, got);
}

#[test]
fn tracks_a_half_percent_clock_error_without_slipping() {
    const TRUE_SPS: f64 = 4.02;
    let syms = symbols(3_000, 0x0f0f_1234);
    let signal = bpsk(&syms, TRUE_SPS, 0.1);
    let mut sync = SymbolSync::new(4.0, 0.02);
    let mut out = Vec::new();
    sync.process(&signal, &mut out);

    assert!(
        (sync.sps() - TRUE_SPS).abs() < 5e-3,
        "sps estimate {} did not converge on {TRUE_SPS}",
        sync.sps()
    );
    let ideal = ((signal.len() - 4) as f64 / TRUE_SPS).floor() as i64;
    assert!(
        (out.len() as i64 - ideal).abs() <= 2,
        "recovered {} symbols, ideal {ideal}",
        out.len()
    );
    assert!(
        alignment(&syms, &out, 300).is_some(),
        "symbol stream slipped while tracking"
    );
}

#[test]
fn silence_free_runs_at_the_nominal_rate() {
    let mut sync = SymbolSync::new(4.0, 0.02);
    let mut out = Vec::new();
    sync.process(&vec![ZERO; 4_000], &mut out);
    assert_eq!(sync.sps(), 4.0, "silence must not pull the clock");
    assert!((out.len() as i64 - 999).abs() <= 2, "{} symbols", out.len());
}

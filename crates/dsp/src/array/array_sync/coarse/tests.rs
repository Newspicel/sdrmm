use std::f64::consts::TAU;

use super::*;
use crate::array::array_sync::signals::Gaussian;

const RATE: f64 = 2_400_000.0;
const SHIFT: usize = 480_000;

struct Pair {
    reference: Vec<Complex<f32>>,
    lane: Vec<Complex<f32>>,
    factor: usize,
}

fn carriers(len: usize, lines: &[(f64, f32)]) -> Vec<Complex<f32>> {
    let mut phasors: Vec<(Complex<f64>, Complex<f64>)> = lines
        .iter()
        .map(|&(hz, amplitude)| {
            (
                Complex::new(f64::from(amplitude), 0.0),
                Complex::from_polar(1.0, TAU * hz / RATE),
            )
        })
        .collect();
    (0..len)
        .map(|_| {
            let mut sum = Complex::<f64>::default();
            for (phasor, step) in &mut phasors {
                sum += *phasor;
                *phasor *= *step;
            }
            Complex::new(sum.re as f32, sum.im as f32)
        })
        .collect()
}

fn decimated_pair(cfo_hz: f64, lines: &[(f64, f32)], seed: u64) -> Pair {
    let factor = coarse_decimation(RATE, COARSE_LAGS);
    let raw = (COARSE_FRAME + 2 * COARSE_LAGS) * factor;
    let mut noise = Gaussian::new(seed);
    let tones = carriers(raw + SHIFT, lines);
    let common: Vec<Complex<f32>> = tones.iter().map(|tone| tone + noise.sample()).collect();
    let step = Complex::from_polar(1.0, TAU * cfo_hz / RATE);
    let mut rotation = Complex::<f64>::new(1.0, 0.0);
    let mut boxcar = Boxcar::new(2, factor);
    let mut out: Vec<Vec<Complex<f32>>> =
        (0..2).map(|_| Vec::with_capacity(raw / factor)).collect();
    let mut reference = Vec::new();
    let mut lane = Vec::new();
    for start in (0..raw).step_by(65_536) {
        let end = (start + 65_536).min(raw);
        reference.clear();
        lane.clear();
        for n in start..end {
            reference.push(common[n + SHIFT] + noise.sample() * 0.5);
            let turn = Complex::new(rotation.re as f32, rotation.im as f32);
            lane.push((common[n] + noise.sample() * 0.5) * turn);
            rotation *= step;
        }
        boxcar.push(&[&reference, &lane], &mut out).unwrap();
    }
    let lane = out.pop().unwrap_or_default();
    let reference = out.pop().unwrap_or_default();
    Pair {
        reference,
        lane,
        factor,
    }
}

#[test]
fn the_smallest_transform_still_searches_for_an_offset() {
    let mut search = CoarseSearch::new(1, 0);
    let flat = [Complex::new(0.0f32, 0.0); 4];
    assert_eq!(
        search.lag_across_clocks(&flat, &flat, 48_000.0),
        Err(CoarseError::NoPeak)
    );
}

#[test]
fn decimation_reaches_a_quarter_second() {
    assert_eq!(coarse_decimation(RATE, COARSE_LAGS), 64);
    assert!(64.0 * COARSE_LAGS as f64 / RATE >= 0.25);
    assert_eq!(coarse_decimation(48_000.0, COARSE_LAGS), 1);
    assert_eq!(coarse_decimation(1e10, COARSE_LAGS), 1024);
    assert_eq!(coarse_decimation(f64::NAN, COARSE_LAGS), 1);
}

#[test]
fn coarse_finds_a_200_ms_lag_on_noise() {
    let pair = decimated_pair(0.0, &[], 1);
    let mut search = CoarseSearch::new(COARSE_FRAME, COARSE_LAGS);
    let found = search.lag(&pair.reference, &pair.lane).unwrap();
    let raw = found.lag * pair.factor as i64;
    assert!(raw.abs_diff(SHIFT as i64) <= pair.factor as u64, "{raw}");
    assert!(found.peak_db > 30.0, "{found:?}");
    assert!(found.second_db > 20.0, "{found:?}");
}

#[test]
fn coarse_finds_a_lag_under_a_1_khz_cfo() {
    let lines = [(-9_000.0, 0.02), (2_100.0, 0.02), (11_000.0, 0.02)];
    let pair = decimated_pair(1_000.0, &lines, 2);
    let mut search = CoarseSearch::new(COARSE_FRAME, COARSE_LAGS);
    assert!(search.lag(&pair.reference, &pair.lane).is_err());
    let rate = RATE / pair.factor as f64;
    let found = search
        .lag_across_clocks(&pair.reference, &pair.lane, rate)
        .unwrap();
    assert_eq!(found.lag * pair.factor as i64, SHIFT as i64);
    assert!((found.cfo_hz - 1_000.0).abs() < 2.0, "{found:?}");
    assert!(found.peak_db > 20.0, "{found:?}");
}

#[test]
fn coarse_finds_a_lag_on_bare_noise_under_a_1_khz_cfo() {
    let pair = decimated_pair(1_000.0, &[], 4);
    let mut search = CoarseSearch::new(COARSE_FRAME, COARSE_LAGS);
    let rate = RATE / pair.factor as f64;
    let found = search
        .lag_across_clocks(&pair.reference, &pair.lane, rate)
        .unwrap();
    assert_eq!(found.lag * pair.factor as i64, SHIFT as i64);
    assert!((found.cfo_hz - 1_000.0).abs() < 2.0, "{found:?}");
    assert!(found.peak_db > 20.0, "{found:?}");
}

#[test]
fn lanes_on_one_clock_measure_no_cfo() {
    let pair = decimated_pair(0.0, &[], 3);
    let mut search = CoarseSearch::new(COARSE_FRAME, COARSE_LAGS);
    let rate = RATE / pair.factor as f64;
    let found = search
        .lag_across_clocks(&pair.reference, &pair.lane, rate)
        .unwrap();
    assert!((found.lag * pair.factor as i64).abs_diff(SHIFT as i64) <= pair.factor as u64);
    assert!(found.cfo_hz.abs() < 0.5, "{found:?}");
}

#[test]
fn coarse_refuses_a_periodic_signal_as_ambiguous() {
    let burst = Gaussian::new(31).block(4_096);
    let mut search = CoarseSearch::new(COARSE_FRAME, COARSE_LAGS);
    let reference: Vec<Complex<f32>> = (0..search.span()).map(|n| burst[n % 4_096]).collect();
    let lane: Vec<Complex<f32>> = (0..search.span())
        .map(|n| burst[(n + 4_096 - 1_000) % 4_096])
        .collect();
    assert_eq!(search.lag(&reference, &lane), Err(CoarseError::Ambiguous));
}

#[test]
fn a_short_capture_or_a_bad_rate_is_refused() {
    let mut search = CoarseSearch::new(1_024, 256);
    let short = vec![Complex::new(1.0, 0.0); search.span() - 1];
    let long = vec![Complex::new(1.0, 0.0); search.span()];
    assert_eq!(search.lag(&short, &long), Err(CoarseError::Short));
    assert_eq!(search.lag(&long, &short), Err(CoarseError::Short));
    assert_eq!(
        search.lag_across_clocks(&long, &long, f64::NAN),
        Err(CoarseError::Rate)
    );
    let silence = vec![Complex::default(); search.span()];
    assert_eq!(search.lag(&silence, &silence), Err(CoarseError::NoPeak));
}

#[test]
fn a_lagging_lane_has_a_positive_lag() {
    let mut search = CoarseSearch::new(4_096, 1_024);
    let common = Gaussian::new(33).block(search.span() + 600);
    let early = &common[600..600 + search.span()];
    let late = &common[..search.span()];
    assert_eq!(search.lag(early, late).map(|found| found.lag), Ok(600));
    assert_eq!(search.lag(late, early).map(|found| found.lag), Ok(-600));
}

#[test]
fn unrelated_lanes_have_no_peak() {
    let mut noise = Gaussian::new(32);
    let mut search = CoarseSearch::new(4_096, 1_024);
    let reference = noise.block(search.span());
    let lane = noise.block(search.span());
    assert_eq!(search.lag(&reference, &lane), Err(CoarseError::NoPeak));
}

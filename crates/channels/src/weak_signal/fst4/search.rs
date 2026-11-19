use std::f32::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::math::fft::Transform;

use super::{SYMBOLS, SYNC_A, SYNC_B, SYNC_STARTS, Timing};

const NOISE_MARGIN_HZ: f32 = 100.0;
const BASELINE_PERCENTILE: usize = 30;
const BASELINE_SEGMENTS: usize = 8;
const BASELINE_SEGMENT_HZ: f32 = 50.0;
const BASELINE_LIFT_DB: f64 = 0.2;
const CLEAN_MODEL: [f32; 7] = [0.25, 0.5, 0.75, 1.0, 0.75, 0.5, 0.25];
const CLEAN_GAIN: f32 = 0.9;
const MAX_CANDIDATES: usize = 200;
const COARSE_STEPS: i32 = 12;
const COARSE_STEP_BAUD: f32 = 0.1;
const COARSE_TIME_STEP: usize = 4;
const FINE_STEPS: i32 = 7;
const FINE_STEP_BAUD: f32 = 0.02;
const FINE_TIME_SPAN: usize = 4;
const SYNC_SYMBOLS: usize = 8;
const KEEP_BANDWIDTH: f32 = 1.3 * 4.0 / 2.0;

/// A frequency where the spectrum shows four tones' worth of energy over the noise.
#[derive(Clone, Copy, Debug)]
pub(super) struct Candidate {
    pub(super) center_hz: f32,
    pub(super) noise: f32,
}

/// Where the sync search placed a candidate: its centre frequency and the frame's first sample
/// at the baseband rate.
#[derive(Clone, Copy, Debug)]
pub(super) struct Synced {
    pub(super) center_hz: f32,
    pub(super) start: usize,
}

/// Finds candidates between `low_hz` and `high_hz` (the lowest tone's frequency) in the
/// spectrum of the whole window, as WSJT-X's `get_candidates_fst4`.
pub(super) fn candidates(
    spectrum: &[Complex<f32>],
    timing: &Timing,
    low_hz: f32,
    high_hz: f32,
    min_sync: f32,
    limit: usize,
) -> Vec<Candidate> {
    let baud = timing.baud();
    let bin_hz = timing.rate() / timing.fft_samples as f32;
    let (lowest_hz, highest_hz) = timing.usable_hz();
    let step_hz = baud / 2.0;
    let summed = ((step_hz / bin_hz) as usize).max(1);
    let index = |hz: f32| (hz / step_hz).round() as usize;
    let noise_low = index((low_hz - NOISE_MARGIN_HZ).max(lowest_hz)).max(4);
    let noise_high = index((high_hz + NOISE_MARGIN_HZ).min(highest_hz));
    let last_bin = spectrum.len().saturating_sub(1);
    let mut power = vec![0.0f32; noise_high + 4];
    for (slot, value) in power
        .iter_mut()
        .enumerate()
        .take(noise_high + 1)
        .skip(noise_low)
    {
        let center = (slot as f32 * step_hz / bin_hz).round() as usize;
        let from = center.saturating_sub(summed / 2);
        let to = (center + summed / 2).min(last_bin);
        *value = spectrum[from.min(to)..=to]
            .iter()
            .map(Complex::norm_sqr)
            .sum();
    }
    let mut tones = vec![0.0f32; power.len()];
    for at in noise_low..=noise_high {
        tones[at] = power[at - 3] + power[at - 1] + power[at + 1] + power[at + 3];
    }
    if noise_high < noise_low + 7 + BASELINE_SEGMENTS {
        return Vec::new();
    }
    let fit_low = noise_low + 3;
    let fit_high = noise_high - 3;
    let segment = (BASELINE_SEGMENT_HZ / step_hz).round() as usize;
    let Some(baseline) = baseline(&tones, fit_low, fit_high, segment) else {
        return Vec::new();
    };
    for at in fit_low..=fit_high {
        tones[at] /= baseline[at];
    }
    let search_low = index((low_hz + 1.5 * baud).max(lowest_hz)).max(fit_low);
    let search_high = index((high_hz + 1.5 * baud).min(highest_hz)).min(fit_high);
    clean(
        &mut tones,
        &baseline,
        search_low,
        search_high,
        step_hz,
        min_sync,
        limit,
    )
}

fn clean(
    tones: &mut [f32],
    baseline: &[f32],
    low: usize,
    high: usize,
    step_hz: f32,
    min_sync: f32,
    limit: usize,
) -> Vec<Candidate> {
    let mut found = Vec::new();
    if low > high {
        return found;
    }
    while found.len() < limit.min(MAX_CANDIDATES) {
        let Some((peak, &strength)) = tones[low..=high]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
        else {
            break;
        };
        if strength < min_sync {
            break;
        }
        let peak = peak + low;
        for (offset, &model) in (-3i32..=3).zip(&CLEAN_MODEL) {
            let at = peak as i32 + 2 * offset;
            if (low as i32..=high as i32).contains(&at) {
                let slot = &mut tones[at as usize];
                *slot = (*slot - CLEAN_GAIN * strength * model).max(0.0);
            }
        }
        found.push(Candidate {
            center_hz: step_hz * peak as f32,
            noise: baseline[peak],
        });
    }
    found
}

/// The noise floor in decibels: in each segment the mean of the lowest points, as WSJT-X's
/// `fst4_baseline` fits them, joined by straight lines. Segments of fixed width rather than one
/// quadratic over the whole band, so a receiver's filter edges do not bend the floor elsewhere.
fn baseline(values: &[f32], low: usize, high: usize, segment: usize) -> Option<Vec<f32>> {
    let span = high + 1 - low;
    let segments = (span / segment.max(1)).max(BASELINE_SEGMENTS);
    let length = span / segments;
    if length == 0 {
        return None;
    }
    let mut knots = Vec::with_capacity(segments);
    for index in 0..segments {
        let from = low + index * length;
        let to = if index + 1 == segments {
            high + 1
        } else {
            from + length
        };
        let mut decibels: Vec<f64> = values[from..to]
            .iter()
            .map(|&value| 10.0 * f64::from(value.max(f32::MIN_POSITIVE)).log10())
            .collect();
        decibels.sort_by(f64::total_cmp);
        let rank = ((decibels.len() * BASELINE_PERCENTILE) as f64 / 100.0).round() as usize;
        let lower = &decibels[..rank.clamp(1, decibels.len())];
        let level = lower.iter().sum::<f64>() / lower.len() as f64;
        knots.push(((from + to - 1) as f64 / 2.0, level));
    }
    let mut base = vec![1.0f32; values.len()];
    for (at, slot) in base.iter_mut().enumerate().take(high + 1).skip(low) {
        let level = interpolate(&knots, at as f64);
        *slot = 10f64.powf((level + BASELINE_LIFT_DB) / 10.0) as f32;
    }
    Some(base)
}

fn interpolate(knots: &[(f64, f64)], at: f64) -> f64 {
    let after = knots.partition_point(|&(x, _)| x < at);
    match (
        after.checked_sub(1).map(|index| knots[index]),
        knots.get(after),
    ) {
        (Some((x0, y0)), Some(&(x1, y1))) => y0 + (y1 - y0) * (at - x0) / (x1 - x0),
        (Some((_, y)), None) | (None, Some(&(_, y))) => y,
        (None, None) => 0.0,
    }
}

/// Moves `center_hz` to zero and decimates to the baseband rate, as `fst4_downsample`.
pub(super) fn downsample(
    spectrum: &[Complex<f32>],
    timing: &Timing,
    center_hz: f32,
    inverse: &mut Transform,
    out: &mut Vec<Complex<f32>>,
) {
    let samples = timing.baseband_samples();
    let bin_hz = timing.rate() / timing.fft_samples as f32;
    let center = (center_hz / bin_hz).round().max(0.0) as usize;
    let edge = ((center_hz + KEEP_BANDWIDTH * timing.baud()) / bin_hz).round() as usize;
    let width = edge.saturating_sub(center) + 1;
    out.clear();
    out.resize(samples, Complex::default());
    let scale = 1.0 / samples as f32;
    if let Some(&value) = spectrum.get(center) {
        out[0] = value * scale;
    }
    for offset in 1..=width.min(samples / 2) {
        if let Some(&value) = spectrum.get(center + offset) {
            out[offset] = value * scale;
        }
        if let Some(below) = center.checked_sub(offset) {
            out[samples - offset] = spectrum[below] * scale;
        }
    }
    inverse.process(out);
}

/// The sync waveforms of the two words, continuous phase across their eight symbols.
pub(super) fn sync_waveforms(per_symbol: usize) -> [Vec<Complex<f32>>; 2] {
    [&SYNC_A, &SYNC_B].map(|word| {
        let mut phase = 0.0f32;
        let mut wave = Vec::with_capacity(SYNC_SYMBOLS * per_symbol);
        for &tone in word {
            let step = (f32::from(tone) - 1.5) * TAU / per_symbol as f32;
            for _ in 0..per_symbol {
                wave.push(Complex::from_polar(1.0, phase));
                phase = (phase + step) % TAU;
            }
        }
        wave
    })
}

/// Searches frequency within a baud of the candidate and the frame start over every position
/// that keeps the whole frame in the window, coarse then fine, as `fst4_sync_search`.
pub(super) fn sync_search(
    baseband: &[Complex<f32>],
    timing: &Timing,
    waves: &[Vec<Complex<f32>>; 2],
) -> Option<(f32, usize)> {
    let per_symbol = timing.baseband_symbol();
    let last = baseband.len().checked_sub(SYMBOLS * per_symbol)?;
    let baud = timing.baud();
    let rate = timing.baseband_rate();
    let coarse = best(
        baseband,
        waves,
        per_symbol,
        rate,
        (-COARSE_STEPS..=COARSE_STEPS).map(|step| step as f32 * COARSE_STEP_BAUD * baud),
        &(0..=last).step_by(COARSE_TIME_STEP),
    )?;
    let fine = best(
        baseband,
        waves,
        per_symbol,
        rate,
        (-FINE_STEPS..=FINE_STEPS).map(|step| coarse.0 + step as f32 * FINE_STEP_BAUD * baud),
        &(coarse.1.saturating_sub(FINE_TIME_SPAN)..=(coarse.1 + FINE_TIME_SPAN).min(last)),
    )?;
    Some(fine)
}

fn best(
    baseband: &[Complex<f32>],
    waves: &[Vec<Complex<f32>>; 2],
    per_symbol: usize,
    rate: f32,
    offsets: impl Iterator<Item = f32>,
    starts: &(impl Iterator<Item = usize> + Clone),
) -> Option<(f32, usize)> {
    let length = waves[0].len();
    let mut found: Option<(f32, usize, f32)> = None;
    let mut templates = [
        vec![Complex::default(); length],
        vec![Complex::default(); length],
    ];
    for offset_hz in offsets {
        let turn = TAU * offset_hz / rate;
        for (template, wave) in templates.iter_mut().zip(waves) {
            for (index, (slot, &sample)) in template.iter_mut().zip(wave).enumerate() {
                *slot = (sample * Complex::from_polar(1.0, (turn * index as f32) % TAU)).conj();
            }
        }
        for start in starts.clone() {
            let power: f32 = SYNC_STARTS
                .iter()
                .enumerate()
                .map(|(block, &symbol)| {
                    let at = start + symbol * per_symbol;
                    baseband[at..at + length]
                        .iter()
                        .zip(&templates[block % 2])
                        .map(|(sample, template)| sample * template)
                        .sum::<Complex<f32>>()
                        .norm()
                })
                .sum();
            if found.is_none_or(|(_, _, strongest)| power > strongest) {
                found = Some((offset_hz, start, power));
            }
        }
    }
    found.map(|(offset, start, _)| (offset, start))
}

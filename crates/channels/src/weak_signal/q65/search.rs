//! Coarse search: spectra at eight steps per symbol, and the tones whose sync pattern stands
//! out at some start time.

use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};
use sdrmm_dsp::math::stats::quantile;

use super::{Geometry, SAMPLE_RATE, SYMBOLS, SYNC};

pub(super) const STEPS_PER_SYMBOL: usize = 8;
const TOP_HZ: f32 = 5_000.0;
const LOWEST_HZ: f32 = 100.0;
const HIGHEST_HZ: f32 = 4_900.0;
const MIN_SYNC_SNR: f32 = 6.0;
const EARLIEST_START_S: f32 = -1.0;
const LATE_MARGIN_S: f32 = 0.5;

/// Power spectra of symbol-long slices taken every eighth of a symbol.
pub(super) struct Spectra {
    pub(super) bins: usize,
    pub(super) steps: usize,
    pub(super) bin_hz: f32,
    pub(super) step_s: f32,
    power: Vec<f32>,
}

impl Spectra {
    pub(super) fn at(&self, bin: usize, step: isize) -> Option<f32> {
        let step = usize::try_from(step)
            .ok()
            .filter(|&step| step < self.steps)?;
        (bin < self.bins).then(|| self.power[step * self.bins + bin])
    }

    fn row(&self, step: usize) -> &[f32] {
        &self.power[step * self.bins..(step + 1) * self.bins]
    }
}

pub(super) struct Planner {
    fft: Arc<dyn RealToComplex<f32>>,
}

impl Planner {
    pub(super) fn new(symbol_samples: usize) -> Self {
        Self {
            fft: RealFftPlanner::<f32>::new().plan_fft_forward(symbol_samples),
        }
    }

    pub(super) fn spectra(&self, samples: &[f32], geometry: &Geometry) -> Spectra {
        let nsps = geometry.symbol_samples;
        let step = nsps / STEPS_PER_SYMBOL;
        let bin_hz = SAMPLE_RATE / nsps as f32;
        let bins = ((TOP_HZ / bin_hz) as usize + 1).min(nsps / 2 + 1);
        let steps = samples.len().saturating_sub(nsps) / step + 1;
        let mut power = vec![0.0; bins * steps];
        let mut input = self.fft.make_input_vec();
        let mut output = self.fft.make_output_vec();
        let mut scratch = self.fft.make_scratch_vec();
        for (index, row) in power.chunks_exact_mut(bins).enumerate() {
            let start = index * step;
            input.fill(0.0);
            let available = samples.len().saturating_sub(start).min(nsps);
            input[..available].copy_from_slice(&samples[start..start + available]);
            if self
                .fft
                .process_with_scratch(&mut input, &mut output, &mut scratch)
                .is_err()
            {
                continue;
            }
            for (cell, value) in row.iter_mut().zip(&output) {
                *cell = value.norm_sqr();
            }
            for _ in 0..geometry.smoothing_passes() {
                smooth_121(row);
            }
        }
        Spectra {
            bins,
            steps,
            bin_hz,
            step_s: step as f32 / SAMPLE_RATE,
            power,
        }
    }
}

fn smooth_121(row: &mut [f32]) {
    let Some(last) = row.len().checked_sub(1) else {
        return;
    };
    let mut previous = row[0];
    for index in 1..last {
        let current = row[index];
        row[index] = 0.5 * current + 0.25 * (previous + row[index + 1]);
        previous = current;
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Candidate {
    pub(super) start_s: f32,
    pub(super) frequency_hz: f32,
}

/// The tones in `low_hz..=high_hz` whose 22 sync symbols stand out at some start between one
/// second before the window and the latest start that still fits, strongest first.
pub(super) fn candidates(
    spectra: &Spectra,
    geometry: &Geometry,
    window_s: f32,
    low_hz: f32,
    high_hz: f32,
    limit: usize,
) -> Vec<Candidate> {
    let first = (low_hz.max(LOWEST_HZ) / spectra.bin_hz).round() as usize;
    let last = ((high_hz.min(HIGHEST_HZ) / spectra.bin_hz).round() as usize).min(spectra.bins - 1);
    if first >= last {
        return Vec::new();
    }
    let earliest = (EARLIEST_START_S / spectra.step_s).floor() as isize;
    let latest =
        ((window_s - geometry.transmission_s() + LATE_MARGIN_S) / spectra.step_s).ceil() as isize;
    let mut best = vec![(f32::MIN, 0isize); spectra.bins];
    for (bin, slot) in best.iter_mut().enumerate().take(last + 1).skip(first) {
        *slot = best_start(spectra, bin, earliest, latest.max(earliest));
    }
    pick(spectra, geometry, &best, first, last, limit)
}

fn best_start(spectra: &Spectra, bin: usize, earliest: isize, latest: isize) -> (f32, isize) {
    let average = (0..spectra.steps)
        .map(|step| spectra.row(step)[bin])
        .sum::<f32>()
        * SYNC.len() as f32
        / spectra.steps as f32;
    let mut best = (f32::MIN, 0);
    for start in earliest..=latest {
        let sum: f32 = SYNC
            .iter()
            .filter_map(|&symbol| spectra.at(bin, start + (STEPS_PER_SYMBOL * symbol) as isize))
            .sum();
        let score = sum - average;
        if score > best.0 {
            best = (score, start);
        }
    }
    best
}

fn pick(
    spectra: &Spectra,
    geometry: &Geometry,
    best: &[(f32, isize)],
    first: usize,
    last: usize,
    limit: usize,
) -> Vec<Candidate> {
    let mut scores: Vec<f32> = best[first..=last].iter().map(|&(score, _)| score).collect();
    let (Some(middle), Some(upper)) = (quantile(&mut scores, 0.5), quantile(&mut scores, 0.84))
    else {
        return Vec::new();
    };
    let spread = (upper - middle).max(f32::MIN_POSITIVE);
    let mut order: Vec<usize> = (first..=last).collect();
    order.sort_by(|&a, &b| best[b].0.total_cmp(&best[a].0));
    let reach = geometry.tone_bins();
    let mut found = Vec::new();
    for bin in order {
        let snr = (best[bin].0 - middle) / spread;
        if snr < MIN_SYNC_SNR || found.len() >= limit {
            break;
        }
        let neighbours = bin.saturating_sub(reach)..=(bin + reach).min(last);
        if neighbours
            .into_iter()
            .any(|other| best[other].0 > best[bin].0)
        {
            continue;
        }
        found.push(Candidate {
            start_s: best[bin].1 as f32 * spectra.step_s,
            frequency_hz: bin as f32 * spectra.bin_hz,
        });
    }
    found
}

/// The SNR in 2500 Hz of a decoded transmission, from the energy its tones gather above the
/// noise once all 85 symbols are lined up.
pub(super) fn snr_db(
    spectra: &Spectra,
    geometry: &Geometry,
    tones: &[u8; SYMBOLS],
    start_s: f32,
    frequency_hz: f32,
) -> f32 {
    let start = (start_s / spectra.step_s).round() as isize;
    let spacing = geometry.tone_bins();
    let mut lined = vec![0.0f32; spectra.bins];
    for (symbol, &tone) in tones.iter().enumerate() {
        let step = start + (STEPS_PER_SYMBOL * symbol) as isize;
        let shift = spacing * usize::from(tone);
        for (bin, slot) in lined.iter_mut().enumerate() {
            if let Some(power) = spectra.at(bin + shift, step) {
                *slot += power;
            }
        }
    }
    let center = (frequency_hz / spectra.bin_hz).round() as usize;
    let edge = (10 * spacing).max((50.0 / spectra.bin_hz).round() as usize);
    let low = center.saturating_sub(2 * edge);
    let high = (center + 2 * edge).min(spectra.bins - 1);
    if high < low + 2 * edge {
        return f32::NAN;
    }
    let noise = (lined[low..low + edge].iter().sum::<f32>()
        + lined[high + 1 - edge..=high].iter().sum::<f32>())
        / (2 * edge) as f32;
    if noise <= 0.0 {
        return f32::NAN;
    }
    let area: f32 = lined[low + edge..=high - edge]
        .iter()
        .map(|power| power / noise - 1.0)
        .sum();
    10.0 * area.max(1.0).log10() - 10.0 * (2_500.0 / spectra.bin_hz).log10()
}

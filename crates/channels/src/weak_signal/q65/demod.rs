//! Fine demodulation: the window as an analytic signal at half rate, shifted to a candidate's
//! sync tone, and its data symbols' spectra lined up for the codec.

use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::math::{fft::Transform, stats::quantile};

use super::{
    Geometry, SAMPLE_RATE, SYMBOLS, SYNC,
    qra::{ALPHABET, CODEWORD_SYMBOLS},
};

pub(super) const HALF_RATE: f32 = SAMPLE_RATE / 2.0;
const PASSBAND_PERCENTILE: f32 = 0.45;
const PASSBAND_REACH: usize = 25;
const LEVEL_PERCENTILE: f32 = 0.40;
const LEVEL_CEILING: f32 = 20.0;
const BIRDIE_HITS: usize = 15;

/// The window as an analytic signal at 6000 samples per second.
pub(super) fn analytic(samples: &[f32]) -> Vec<Complex<f32>> {
    let len = samples.len() - samples.len() % 2;
    if len == 0 {
        return Vec::new();
    }
    let mut spectrum: Vec<Complex<f32>> = samples[..len]
        .iter()
        .map(|&s| Complex::new(s, 0.0))
        .collect();
    Transform::forward(len).process(&mut spectrum);
    let half = len / 2;
    let mut folded: Vec<Complex<f32>> = spectrum[..half].to_vec();
    for value in &mut folded[half / 2 + 1..] {
        *value = Complex::new(0.0, 0.0);
    }
    folded[0] *= 0.5;
    Transform::inverse(half).process(&mut folded);
    let scale = 2.0 / len as f32;
    for value in &mut folded {
        *value *= scale;
    }
    folded
}

/// `signal` moved down by `hz`.
pub(super) fn shifted(signal: &[Complex<f32>], hz: f32, out: &mut Vec<Complex<f32>>) {
    out.clear();
    let step = -TAU * f64::from(hz) / f64::from(HALF_RATE);
    out.extend(signal.iter().enumerate().map(|(index, &sample)| {
        let phase = step * index as f64;
        sample * Complex::new(phase.cos() as f32, phase.sin() as f32)
    }));
}

/// Per data symbol, the energies of `64 * (2 + tone bins)` bins starting 64 tone bins below the
/// first data tone, for a signal starting at `start` samples of the shifted half-rate signal.
pub(super) struct SymbolSpectra {
    fft: Transform,
    buffer: Vec<Complex<f32>>,
    column: Vec<f32>,
}

impl SymbolSpectra {
    pub(super) fn new(geometry: &Geometry) -> Self {
        let half_symbol = geometry.symbol_samples / 2;
        Self {
            fft: Transform::forward(half_symbol),
            buffer: vec![Complex::new(0.0, 0.0); half_symbol],
            column: Vec::with_capacity(CODEWORD_SYMBOLS),
        }
    }

    pub(super) fn energies(
        &mut self,
        signal: &[Complex<f32>],
        geometry: &Geometry,
        start: isize,
        energies: &mut Vec<f32>,
    ) {
        let half_symbol = geometry.symbol_samples / 2;
        let row = geometry.row_bins();
        let spacing = geometry.tone_bins();
        energies.clear();
        energies.resize(row * CODEWORD_SYMBOLS, 0.0);
        let data = (0..SYMBOLS - 1).filter(|symbol| !SYNC.contains(symbol));
        for (index, symbol) in data.enumerate() {
            let first = start + (symbol * half_symbol) as isize;
            for (offset, slot) in self.buffer.iter_mut().enumerate() {
                *slot = usize::try_from(first + offset as isize)
                    .ok()
                    .and_then(|at| signal.get(at))
                    .copied()
                    .unwrap_or_default();
            }
            self.fft.process(&mut self.buffer);
            let out = &mut energies[index * row..(index + 1) * row];
            for (bin, slot) in out.iter_mut().enumerate() {
                let frequency = bin as isize - ALPHABET as isize + spacing as isize;
                let wrapped = frequency.rem_euclid(half_symbol as isize) as usize;
                *slot = self.buffer[wrapped].norm_sqr();
            }
        }
        self.equalize(energies, row);
        level(energies);
        zap_birdies(energies, row);
    }

    fn equalize(&mut self, energies: &mut [f32], row: usize) {
        let mut floor = vec![0.0f32; row];
        for (bin, value) in floor.iter_mut().enumerate() {
            self.column.clear();
            self.column
                .extend((0..CODEWORD_SYMBOLS).map(|symbol| energies[symbol * row + bin]));
            *value = quantile(&mut self.column, PASSBAND_PERCENTILE).unwrap_or(0.0);
        }
        let smooth = smoothed_passband(&floor);
        for symbol in energies.chunks_exact_mut(row) {
            for (value, base) in symbol.iter_mut().zip(&smooth) {
                *value /= base + 0.001;
            }
        }
    }
}

fn smoothed_passband(floor: &[f32]) -> Vec<f32> {
    let row = floor.len();
    let reach = PASSBAND_REACH;
    let mut smooth = vec![0.0f32; row];
    if row < 2 * reach {
        smooth.copy_from_slice(floor);
        return smooth;
    }
    let edge =
        |range: std::ops::Range<usize>| floor[range].iter().sum::<f32>() / (reach - 1) as f32;
    let low = edge(0..reach - 1);
    let high = edge(row - reach..row);
    smooth[..reach - 1].fill(low);
    smooth[row - reach..].fill(high);
    for bin in reach - 1..row - reach {
        smooth[bin] =
            floor[bin + 1 - reach..=bin + reach].iter().sum::<f32>() / (2 * reach + 1) as f32;
    }
    smooth
}

fn level(energies: &mut [f32]) {
    let mut scratch = energies.to_vec();
    let base = quantile(&mut scratch, LEVEL_PERCENTILE).unwrap_or(1.0);
    if base <= 0.0 {
        return;
    }
    for value in energies.iter_mut() {
        *value = (*value / base).min(LEVEL_CEILING);
    }
}

/// Flattens bins that are the loudest of more than 15 symbols: a steady carrier, not data.
fn zap_birdies(energies: &mut [f32], row: usize) {
    let mut hits = vec![0usize; row];
    for symbol in energies.chunks_exact(row) {
        let peak = symbol
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map_or(0, |(bin, _)| bin);
        hits[peak] += 1;
    }
    for (bin, &count) in hits.iter().enumerate() {
        if count > BIRDIE_HITS {
            for symbol in energies.chunks_exact_mut(row) {
                symbol[bin] = 1.0;
            }
        }
    }
}

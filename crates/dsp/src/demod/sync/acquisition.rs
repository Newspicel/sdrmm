use std::f64::consts::TAU;

use num_complex::Complex;

/// How much of the envelope must sit on the symbol-rate line before an acquisition is trusted.
/// Below it the opening symbols are too noisy to place, and the loop keeps its own timing.
const MIN_LINE_SHARE: f64 = 0.0;

/// Spreads of the mean data imbalance alone produces, below which the mean is not taken for an
/// offset.
const IMBALANCE_SPREADS: f32 = 1.0;

/// A feed-forward timing estimate taken over the opening samples of every burst, so the loop
/// starts on the symbol centres instead of pulling in from wherever it free-ran to.
#[derive(Clone, Copy, Debug)]
pub(super) struct Acquisition {
    pub(super) span: usize,
    /// Samples kept behind the next one, so a burst's first centre may lie up to half a symbol
    /// back.
    pub(super) reach: usize,
    pub(super) armed: bool,
}

impl Acquisition {
    pub(super) fn new(symbols: usize, sps: f64) -> Self {
        Self {
            span: (symbols as f64 * sps).ceil() as usize,
            reach: (0.5 * sps).ceil() as usize + 1,
            armed: true,
        }
    }
}

/// Where in `window`, as a sample offset in [0, sps), the envelope's symbol-rate line peaks
/// (Oerder and Meyr). The magnitude, not its square, keeps four-level data with a small
/// rolloff from throwing the estimate half a symbol out. `None` when the line is too weak.
pub(super) fn symbol_peak(window: &[Complex<f32>], sps: f64) -> Option<f64> {
    let offset = carrier_offset(window, window.len() as f32 / sps as f32);
    let step = Complex::from_polar(1.0, -TAU / sps);
    let mut rot = Complex::new(1.0f64, 0.0);
    let mut line = Complex::new(0.0f64, 0.0);
    let mut total = 0.0f64;
    for s in window {
        let mag = f64::from((s - offset).norm());
        line += rot * mag;
        total += mag;
        rot *= step;
    }
    if !(line.norm() > MIN_LINE_SHARE * total && line.arg().is_finite()) {
        return None;
    }
    Some((-line.arg() / TAU).rem_euclid(1.0) * sps)
}

pub(super) fn symbol_peak_real(window: &[f32], sps: f64) -> Option<f64> {
    let offset = carrier_offset_real(window, window.len() as f32 / sps as f32);
    let step = Complex::from_polar(1.0, -TAU / sps);
    let mut rot = Complex::new(1.0f64, 0.0);
    let mut line = Complex::new(0.0f64, 0.0);
    let mut total = 0.0f64;
    for &s in window {
        let mag = f64::from((s - offset).abs());
        line += rot * mag;
        total += mag;
        rot *= step;
    }
    if !(line.norm() > MIN_LINE_SHARE * total && line.arg().is_finite()) {
        return None;
    }
    Some((-line.arg() / TAU).rem_euclid(1.0) * sps)
}

fn carrier_offset_real(window: &[f32], symbols: f32) -> f32 {
    let n = window.len().max(1) as f32;
    let mean = window.iter().copied().sum::<f32>() / n;
    let rms = (window.iter().map(|&s| (s - mean) * (s - mean)).sum::<f32>() / n).sqrt();
    let imbalance = IMBALANCE_SPREADS * rms / symbols.max(1.0).sqrt();
    let size = mean.abs();
    if size <= imbalance {
        0.0
    } else {
        mean * ((size - imbalance) / size)
    }
}

/// The carrier offset under `window`: its mean, less the part a run of unbalanced symbols could
/// produce on its own, so only an offset the data cannot explain moves the estimate.
fn carrier_offset(window: &[Complex<f32>], symbols: f32) -> Complex<f32> {
    let n = window.len().max(1) as f32;
    let mean = window.iter().sum::<Complex<f32>>() / n;
    let rms = (window.iter().map(|s| (s - mean).norm_sqr()).sum::<f32>() / n).sqrt();
    let imbalance = IMBALANCE_SPREADS * rms / symbols.max(1.0).sqrt();
    let size = mean.norm();
    if size <= imbalance {
        Complex::default()
    } else {
        mean * ((size - imbalance) / size)
    }
}

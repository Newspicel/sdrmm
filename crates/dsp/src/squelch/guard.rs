use std::ops::Range;

use num_complex::Complex;

use crate::{ddc::flat_bandwidth_hz, fft::Transform, iir::one_pole_coeff, window::blackman_harris};

const LEN: usize = 128;
const MAIN_LOBE_BINS: f64 = 4.0;
const GAP_FRACTION: f64 = 0.1;
const MIN_BINS: usize = 4;
const TAU_S: f64 = 10e-3;

#[derive(Clone)]
pub(super) struct GuardMeter {
    fft: Transform,
    window: Vec<f32>,
    segment: Vec<Complex<f32>>,
    filled: usize,
    sides: [Option<Range<i64>>; 2],
    coeff: f32,
    level: f32,
}

impl std::fmt::Debug for GuardMeter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GuardMeter")
            .field("sides", &self.sides)
            .field("level", &self.level)
            .finish_non_exhaustive()
    }
}

impl GuardMeter {
    pub(super) fn new(rate: f64, band_low_hz: f64, band_high_hz: f64) -> Option<Self> {
        let sides = guard_bins(rate, band_low_hz, band_high_hz);
        sides.iter().any(Option::is_some).then(|| Self {
            fft: Transform::forward(LEN),
            window: blackman_harris(LEN),
            segment: vec![Complex::new(0.0, 0.0); LEN],
            filled: 0,
            sides,
            coeff: one_pole_coeff(rate / LEN as f64, TAU_S),
            level: 0.0,
        })
    }

    pub(super) fn level(&self) -> f32 {
        self.level
    }

    pub(super) fn reset(&mut self) {
        self.filled = 0;
        self.level = 0.0;
    }

    pub(super) fn process(&mut self, wide: &[Complex<f32>]) {
        for &x in wide {
            self.segment[self.filled] = x;
            self.filled += 1;
            if self.filled == LEN {
                self.filled = 0;
                self.measure();
            }
        }
    }

    fn measure(&mut self) {
        for (x, w) in self.segment.iter_mut().zip(&self.window) {
            *x *= *w;
        }
        self.fft.process(&mut self.segment);
        let quietest = self
            .sides
            .iter()
            .flatten()
            .map(|bins| mean_power(&self.segment, bins.clone()))
            .fold(f32::INFINITY, f32::min);
        self.level = if !quietest.is_finite() {
            0.0
        } else if self.level > 0.0 {
            self.level + self.coeff * (quietest - self.level)
        } else {
            quietest
        };
    }
}

fn mean_power(spectrum: &[Complex<f32>], bins: Range<i64>) -> f32 {
    let count = bins.end - bins.start;
    bins.map(|bin| spectrum[bin.rem_euclid(LEN as i64) as usize].norm_sqr())
        .sum::<f32>()
        / count as f32
}

fn guard_bins(rate: f64, band_low_hz: f64, band_high_hz: f64) -> [Option<Range<i64>>; 2] {
    let bin_hz = rate / LEN as f64;
    let edge = flat_bandwidth_hz(rate) / 2.0 / bin_hz - MAIN_LOBE_BINS;
    let gap = MAIN_LOBE_BINS + GAP_FRACTION * (band_high_hz - band_low_hz) / bin_hz;
    let below = (-edge).ceil() as i64..(band_low_hz / bin_hz - gap).floor() as i64 + 1;
    let above = (band_high_hz / bin_hz + gap).ceil() as i64..edge.floor() as i64 + 1;
    [below, above].map(|bins| (bins.end - bins.start >= MIN_BINS as i64).then_some(bins))
}

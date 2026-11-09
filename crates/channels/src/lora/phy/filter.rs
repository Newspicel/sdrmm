use std::{f64::consts::TAU, sync::LazyLock};

use num_complex::Complex;
use sdrmm_dsp::{
    fir::kaiser_beta,
    special::{bessel_i0, sinc},
};

use super::ring::Ring;

pub(crate) const HALF_TAPS: usize = 24;
const TAPS: usize = 2 * HALF_TAPS;
const CUTOFF: f64 = 0.26;
const ATTENUATION_DB: f64 = 60.0;

const TABLE_STEPS: usize = 64;

/// The windowed-sinc prototype sampled every 1/64 of a wide sample from `-HALF_TAPS` to
/// `HALF_TAPS`; a fractional tap is read off it by linear interpolation.
static PROTOTYPE: LazyLock<Vec<f32>> = LazyLock::new(|| {
    let beta = kaiser_beta(ATTENUATION_DB);
    let scale = bessel_i0(beta);
    (0..=2 * HALF_TAPS * TABLE_STEPS + 1)
        .map(|step| {
            let t = step as f64 / TABLE_STEPS as f64 - HALF_TAPS as f64;
            let x = t / HALF_TAPS as f64;
            if x.abs() >= 1.0 {
                return 0.0;
            }
            let window = bessel_i0(beta * (1.0 - x * x).sqrt()) / scale;
            (2.0 * CUTOFF * sinc(2.0 * CUTOFF * t) * window) as f32
        })
        .collect()
});

fn fractional(table: &[f32], mu: f64, out: &mut [f32; 2 * TAPS]) {
    let position = mu.clamp(0.0, 1.0) * TABLE_STEPS as f64;
    let first = (position.floor() as usize).min(TABLE_STEPS - 1);
    let blend = (position - first as f64) as f32;
    let mut sum = 0.0;
    for (k, pair) in out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let at = first + (TAPS - 1 - k) * TABLE_STEPS;
        let tap = table[at] + blend * (table[at + 1] - table[at]);
        *pair = [tap, tap];
        sum += tap;
    }
    for tap in out.iter_mut() {
        *tap /= sum;
    }
}

/// Reads one symbol at one sample per chip from the wide stream, at a fractional start and with
/// the frequency offset removed. Mixing the wide samples first leaves a real fractional-delay
/// filter, the same for every output of the window.
pub(crate) struct Extractor {
    prototype: &'static [f32],
    taps: [f32; 2 * TAPS],
    span: Vec<Complex<f32>>,
    mixed: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Window {
    pub start_chip: f64,
    pub cfo_bins: f64,
    pub inverted: bool,
}

impl Window {
    #[must_use]
    pub(crate) fn last_wide_index(self, chips: usize) -> u64 {
        let end = 2.0 * (self.start_chip + chips as f64);
        (end.ceil().max(0.0) as u64) + HALF_TAPS as u64
    }
}

fn span_len(chips: usize) -> usize {
    2 * chips.saturating_sub(1) + TAPS
}

impl Extractor {
    #[must_use]
    pub(crate) fn new(chips: usize) -> Self {
        let len = span_len(chips);
        Self {
            prototype: LazyLock::force(&PROTOTYPE),
            taps: [0.0; 2 * TAPS],
            span: vec![Complex::new(0.0, 0.0); len],
            mixed: vec![0.0; 2 * len],
        }
    }

    pub(crate) fn extract(&mut self, ring: &Ring, window: Window, out: &mut [Complex<f32>]) {
        let len = span_len(out.len()).min(self.span.len());
        let position = 2.0 * window.start_chip;
        let whole = position.floor();
        fractional(self.prototype, position - whole, &mut self.taps);
        let base = whole as i64 + 1 - HALF_TAPS as i64;
        ring.copy(base, &mut self.span[..len]);
        let cycles = window.cfo_bins / (2.0 * out.len() as f64);
        self.mix(base, cycles, window.inverted, len);
        let outputs = out.len().min((len + 2 - TAPS) / 2);
        let (pairs, rest) = out[..outputs].as_chunks_mut::<2>();
        for (n, pair) in pairs.iter_mut().enumerate() {
            let at = 8 * n;
            *pair = filtered(&self.mixed[at..at + 2 * TAPS + 4], &self.taps);
        }
        if let Some(last) = rest.first_mut() {
            let at = 4 * (outputs - 1);
            *last = filtered::<1>(&self.mixed[at..at + 2 * TAPS], &self.taps)[0];
        }
    }

    /// Removes the frequency offset in runs of [`MIX_RUN`] samples, each anchored on an exact
    /// phase so the single-precision rotation never drifts.
    fn mix(&mut self, base: i64, cycles: f64, inverted: bool, len: usize) {
        let step = Complex::<f32>::from_polar(1.0, (-TAU * cycles) as f32);
        let pairs = self.mixed.as_chunks_mut::<2>().0;
        let runs = pairs[..len]
            .chunks_mut(MIX_RUN)
            .zip(self.span[..len].chunks(MIX_RUN));
        for (run, (pairs, samples)) in runs.enumerate() {
            let turns = -cycles * (base + (run * MIX_RUN) as i64) as f64;
            let mut rotation = Complex::<f32>::from_polar(1.0, (TAU * turns.fract()) as f32);
            for (pair, &sample) in pairs.iter_mut().zip(samples) {
                let sample = if inverted { sample.conj() } else { sample };
                let mixed = sample * rotation;
                *pair = [mixed.re, mixed.im];
                rotation *= step;
            }
        }
    }
}

const MIX_RUN: usize = 64;
const LANES: usize = 16;
const FUSED: bool = cfg!(any(target_arch = "aarch64", target_feature = "fma"));

/// `N` neighbouring outputs from interleaved samples and taps doubled to match, each in
/// independent lanes the compiler keeps in vector registers; the outputs share each tap load.
fn filtered<const N: usize>(samples: &[f32], taps: &[f32; 2 * TAPS]) -> [Complex<f32>; N] {
    let mut acc = [[0.0f32; LANES]; N];
    for (block, weights) in taps.as_chunks::<LANES>().0.iter().enumerate() {
        for (output, acc) in acc.iter_mut().enumerate() {
            let start = block * LANES + 4 * output;
            let Some(values) = samples.get(start..start + LANES) else {
                continue;
            };
            for lane in 0..LANES {
                acc[lane] = if FUSED {
                    values[lane].mul_add(weights[lane], acc[lane])
                } else {
                    acc[lane] + values[lane] * weights[lane]
                };
            }
        }
    }
    acc.map(|acc| {
        let quad: [f32; 4] =
            std::array::from_fn(|i| (acc[i] + acc[i + 4]) + (acc[i + 8] + acc[i + 12]));
        Complex::new(quad[0] + quad[2], quad[1] + quad[3])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(cycles: f64, len: usize) -> Ring {
        let mut ring = Ring::new(len);
        for k in 0..len {
            ring.extend(&[Complex::from_polar(1.0, (TAU * cycles * k as f64) as f32)]);
        }
        ring
    }

    #[test]
    fn a_fractional_window_samples_a_tone_between_its_samples() {
        let ring = tone(0.03, 2048);
        let mut extractor = Extractor::new(4096);
        let mut out = vec![Complex::new(0.0, 0.0); 64];
        let window = Window {
            start_chip: 200.37,
            cfo_bins: 0.0,
            inverted: false,
        };
        extractor.extract(&ring, window, &mut out);
        for (n, sample) in out.iter().enumerate() {
            let t = 2.0 * (200.37 + n as f64);
            let expected = Complex::from_polar(1.0, (TAU * 0.03 * t) as f32);
            assert!((sample - expected).norm() < 0.01, "{n}");
        }
    }

    #[test]
    fn the_frequency_offset_is_removed_before_the_band_limit() {
        let cycles = 0.12;
        let ring = tone(cycles, 4096);
        let mut extractor = Extractor::new(4096);
        let chips = 128;
        let mut out = vec![Complex::new(0.0, 0.0); chips];
        let window = Window {
            start_chip: 500.0,
            cfo_bins: cycles * 2.0 * chips as f64,
            inverted: false,
        };
        extractor.extract(&ring, window, &mut out);
        for sample in &out[1..] {
            assert!((sample - out[0]).norm() < 0.01);
            assert!((sample.norm() - 1.0).abs() < 0.01);
        }
    }

    #[test]
    fn the_band_limit_rejects_what_lies_beyond_the_chirp() {
        let ring = tone(0.4, 2048);
        let mut extractor = Extractor::new(4096);
        let mut out = vec![Complex::new(0.0, 0.0); 64];
        let window = Window {
            start_chip: 300.0,
            cfo_bins: 0.0,
            inverted: false,
        };
        extractor.extract(&ring, window, &mut out);
        assert!(out.iter().all(|s| s.norm() < 0.01));
    }
}

use std::f64::consts::TAU;

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

pub(crate) struct Prototype {
    beta: f64,
    scale: f64,
}

impl Prototype {
    #[must_use]
    pub(crate) fn new() -> Self {
        let beta = kaiser_beta(ATTENUATION_DB);
        Self {
            beta,
            scale: bessel_i0(beta),
        }
    }

    fn value(&self, t: f64) -> f64 {
        let x = t / HALF_TAPS as f64;
        if x.abs() >= 1.0 {
            return 0.0;
        }
        let window = bessel_i0(self.beta * (1.0 - x * x).sqrt()) / self.scale;
        2.0 * CUTOFF * sinc(2.0 * CUTOFF * t) * window
    }

    #[must_use]
    pub(crate) fn centred(&self) -> Vec<f32> {
        let taps: Vec<f64> = (0..=2 * HALF_TAPS)
            .map(|k| self.value(k as f64 - HALF_TAPS as f64))
            .collect();
        let sum: f64 = taps.iter().sum();
        taps.into_iter().map(|t| (t / sum) as f32).collect()
    }

    fn fractional(&self, mu: f64, cycles_per_sample: f64, out: &mut [Complex<f32>; TAPS]) {
        let sum: f64 = (0..TAPS).map(|k| self.value(mu - offset(k))).sum();
        for (k, tap) in out.iter_mut().enumerate() {
            let t = mu - offset(k);
            let weight = self.value(t) / sum;
            *tap = Complex::from_polar(weight as f32, (TAU * cycles_per_sample * t) as f32);
        }
    }
}

fn offset(k: usize) -> f64 {
    k as f64 + 1.0 - HALF_TAPS as f64
}

pub(crate) struct Extractor {
    prototype: Prototype,
    taps: [Complex<f32>; TAPS],
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

impl Extractor {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            prototype: Prototype::new(),
            taps: [Complex::new(0.0, 0.0); TAPS],
        }
    }

    pub(crate) fn extract(&mut self, ring: &Ring, window: Window, out: &mut [Complex<f32>]) {
        let chips = out.len() as f64;
        let position = 2.0 * window.start_chip;
        let whole = position.floor();
        let cycles = window.cfo_bins / (2.0 * chips);
        self.prototype
            .fractional(position - whole, cycles, &mut self.taps);
        let base = whole as i64 + 1 - HALF_TAPS as i64;
        let turns = -cycles * position;
        let mut rotation = Complex::<f64>::from_polar(1.0, TAU * (turns - turns.floor()));
        let step = Complex::<f64>::from_polar(1.0, -TAU * 2.0 * cycles);
        for (n, sample) in out.iter_mut().enumerate() {
            let first = base + 2 * n as i64;
            let mut acc = Complex::new(0.0f32, 0.0f32);
            for (k, &tap) in self.taps.iter().enumerate() {
                let x = ring.at(first + k as i64);
                acc += tap * if window.inverted { x.conj() } else { x };
            }
            *sample = acc * Complex::new(rotation.re as f32, rotation.im as f32);
            rotation *= step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(cycles: f64, len: usize) -> Ring {
        let mut ring = Ring::new(len);
        for k in 0..len {
            ring.push(Complex::from_polar(1.0, (TAU * cycles * k as f64) as f32));
        }
        ring
    }

    #[test]
    fn a_fractional_window_samples_a_tone_between_its_samples() {
        let ring = tone(0.03, 2048);
        let mut extractor = Extractor::new();
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
        let mut extractor = Extractor::new();
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
        let mut extractor = Extractor::new();
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

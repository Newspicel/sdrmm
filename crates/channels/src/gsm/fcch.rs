use std::f64::consts::{FRAC_PI_2, PI, TAU};

use num_complex::Complex;

use super::burst::{BURST_SYMBOLS, INPUT_RATE_HZ, SPS};

const WINDOW: usize = 128 * SPS;
const LAG: usize = SPS;
const THRESHOLD: f64 = 0.5;
const MAX_OFFSET_HZ: f64 = 30_000.0;
const MIN_RUN: usize = 8;
const MAX_RUN: usize = BURST_SYMBOLS * SPS + WINDOW;
const CORE_GUARD: usize = 8 * SPS;
const REFRESH: u64 = 1 << 16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tone {
    pub(crate) start: usize,
    pub(crate) offset_hz: f64,
}

pub(crate) struct ToneSearch {
    product: Complex<f64>,
    energy: f64,
    filled: usize,
    run: Option<(usize, usize)>,
    seen: u64,
}

impl ToneSearch {
    pub(crate) fn new() -> Self {
        Self {
            product: Complex::default(),
            energy: 0.0,
            filled: 0,
            run: None,
            seen: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::new();
    }

    pub(crate) const fn history() -> usize {
        WINDOW + LAG
    }

    pub(crate) fn rebase(&mut self, by: usize) {
        self.run = self
            .run
            .and_then(|(first, last)| Some((first.checked_sub(by)?, last.checked_sub(by)?)));
    }

    pub(crate) fn step(&mut self, samples: &[Complex<f32>], at: usize) -> Option<Tone> {
        if at < LAG {
            return None;
        }
        self.add(samples, at, 1.0);
        if self.filled < WINDOW {
            self.filled += 1;
            return None;
        }
        self.add(samples, at - WINDOW, -1.0);
        self.seen += 1;
        if self.seen.is_multiple_of(REFRESH) {
            self.recompute(samples, at);
        }
        if self.is_tone() {
            self.run = Some(self.run.map_or((at, at), |(first, _)| (first, at)));
            return None;
        }
        let (first, last) = self.run.take()?;
        let length = last - first + 1;
        if !(MIN_RUN..=MAX_RUN).contains(&length) {
            return None;
        }
        let start = (first + last + 1).checked_sub(WINDOW + BURST_SYMBOLS * SPS)? / 2;
        let (offset_hz, coherence) = tone_offset(samples, start)?;
        (coherence >= THRESHOLD).then_some(Tone { start, offset_hz })
    }

    fn add(&mut self, samples: &[Complex<f32>], at: usize, sign: f64) {
        let (now, before) = (samples[at], samples[at - LAG]);
        let product = now * before.conj();
        self.product += Complex::new(f64::from(product.re), f64::from(product.im)) * sign;
        self.energy += f64::from(now.norm_sqr() + before.norm_sqr()) * 0.5 * sign;
    }

    fn recompute(&mut self, samples: &[Complex<f32>], at: usize) {
        self.product = Complex::default();
        self.energy = 0.0;
        for k in at + 1 - WINDOW..=at {
            self.add(samples, k, 1.0);
        }
    }

    fn is_tone(&self) -> bool {
        if self.energy <= 0.0 || self.product.norm() < THRESHOLD * self.energy {
            return false;
        }
        (offset_hz(self.product.arg())).abs() <= MAX_OFFSET_HZ
    }
}

const TONE_RADIANS: f64 = FRAC_PI_2 / SPS as f64;
const FINE_LAGS: [usize; 2] = [16, 128];

fn offset_hz(angle: f64) -> f64 {
    (angle - FRAC_PI_2) * INPUT_RATE_HZ / (TAU * LAG as f64)
}

pub(crate) fn tone_offset(samples: &[Complex<f32>], start: usize) -> Option<(f64, f64)> {
    let core = samples.get(start + CORE_GUARD..start + BURST_SYMBOLS * SPS - CORE_GUARD)?;
    let (product, energy) = lagged(core, LAG);
    if energy <= 0.0 {
        return None;
    }
    let mut radians = product.arg() / LAG as f64;
    for lag in FINE_LAGS {
        let (product, _) = lagged(core, lag);
        let predicted = radians * lag as f64;
        radians += wrap(product.arg() - predicted) / lag as f64;
    }
    let offset = (radians - TONE_RADIANS) * INPUT_RATE_HZ / TAU;
    Some((offset, product.norm() / energy))
}

fn lagged(core: &[Complex<f32>], lag: usize) -> (Complex<f64>, f64) {
    core.iter().zip(&core[lag.min(core.len())..]).fold(
        (Complex::default(), 0.0),
        |(product, energy), (before, now)| {
            let p = now * before.conj();
            (
                product + Complex::new(f64::from(p.re), f64::from(p.im)),
                energy + f64::from(now.norm_sqr() + before.norm_sqr()) * 0.5,
            )
        },
    )
}

fn wrap(angle: f64) -> f64 {
    (angle + PI).rem_euclid(TAU) - PI
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at_snr, frequency_shift};

    #[test]
    fn the_tone_offset_ignores_coloured_noise() {
        let tone = vec![Complex::new(1.0, 0.0); 4 * BURST_SYMBOLS * SPS];
        let shifted = frequency_shift(
            &tone,
            INPUT_RATE_HZ / (4 * SPS) as f64 + 7_000.0,
            INPUT_RATE_HZ,
        );
        let mut filtered = Vec::new();
        crate::gsm::channel_filter().process(&at_snr(&shifted, 0.0, 9), &mut filtered);
        let (offset, coherence) = tone_offset(&filtered, BURST_SYMBOLS * SPS).unwrap();
        assert!((offset - 7_000.0).abs() < 150.0, "{offset}");
        assert!(coherence > THRESHOLD, "{coherence}");
    }
}

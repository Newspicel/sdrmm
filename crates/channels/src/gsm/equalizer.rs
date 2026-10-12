use std::f32::consts::FRAC_PI_2;

use num_complex::Complex;

use super::burst::{BURST_SYMBOLS, SPS, Training};

pub(crate) const TAPS: usize = 5;
const STATES: usize = 1 << (TAPS - 1);
const STATE_MASK: usize = STATES - 1;
const PRECURSORS: usize = 1;
const NOISE_FLOOR: f32 = 1e-12;
const MAX_LLR: f32 = 60.0;
const UNREACHABLE: f32 = f32::NEG_INFINITY;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Demodulated {
    pub(crate) soft: [f32; BURST_SYMBOLS],
    pub(crate) start: usize,
    pub(crate) snr_db: f32,
}

pub(crate) struct Equalizer {
    rotated: Vec<Complex<f32>>,
    correlation: Vec<Complex<f32>>,
    symbols: [Complex<f32>; BURST_SYMBOLS],
    taps: [Complex<f32>; TAPS],
    expected: [[Complex<f32>; 2]; STATES],
    alpha: Vec<[f32; STATES]>,
    beta: [f32; STATES],
    beta_next: [f32; STATES],
}

impl Equalizer {
    pub(crate) fn new() -> Self {
        Self {
            rotated: Vec::with_capacity(1_024),
            correlation: Vec::with_capacity(256),
            symbols: [Complex::<f32>::default(); BURST_SYMBOLS],
            taps: [Complex::<f32>::default(); TAPS],
            expected: [[Complex::<f32>::default(); 2]; STATES],
            alpha: vec![[0.0; STATES]; BURST_SYMBOLS + 1],
            beta: [0.0; STATES],
            beta_next: [0.0; STATES],
        }
    }

    pub(crate) fn reach(search: usize) -> (usize, usize) {
        let before = SPS * PRECURSORS + search;
        let after = SPS * (BURST_SYMBOLS + TAPS) + search;
        (before, after)
    }

    pub(crate) fn demodulate(
        &mut self,
        samples: &[Complex<f32>],
        start: usize,
        search: usize,
        frequency: f32,
        training: &Training,
    ) -> Option<Demodulated> {
        let (before, after) = Self::reach(search);
        let first = start.checked_sub(before)?;
        let window = samples.get(first..start + after)?;
        self.derotate(window, frequency);
        let origin = before - SPS * PRECURSORS;
        let low = origin - search;
        let best = self.estimate(low, origin + search, training);
        self.gather(low, best);
        let noise = self.noise(training).max(NOISE_FLOOR);
        let power: f32 = self.taps.iter().map(Complex::norm_sqr).sum();
        let mut soft = [0.0; BURST_SYMBOLS];
        self.detect(noise, training, &mut soft);
        let main = (0..TAPS)
            .max_by(|&a, &b| self.taps[a].norm_sqr().total_cmp(&self.taps[b].norm_sqr()))
            .unwrap_or(PRECURSORS);
        Some(Demodulated {
            soft,
            start: first + best + SPS * main,
            snr_db: 10.0 * (power / noise).max(NOISE_FLOOR).log10(),
        })
    }

    fn derotate(&mut self, window: &[Complex<f32>], frequency: f32) {
        let step = Complex::from_polar(1.0, -(FRAC_PI_2 / SPS as f32 + frequency));
        let mut phasor = Complex::new(1.0f32, 0.0);
        self.rotated.clear();
        for (k, &sample) in window.iter().enumerate() {
            self.rotated.push(sample * phasor);
            phasor *= step;
            if k % 64 == 63 {
                phasor /= phasor.norm();
            }
        }
    }

    fn estimate(&mut self, low: usize, high: usize, training: &Training) -> usize {
        let core = training.core();
        let scale = 1.0 / core.len() as f32;
        self.correlation.clear();
        for q in low..=high + SPS * (TAPS - 1) {
            let mut sum = Complex::<f32>::default();
            for i in core.clone() {
                sum += self.rotated[q + SPS * (training.at + i)] * training.symbol(i);
            }
            self.correlation.push(sum * scale);
        }
        (0..=high - low)
            .max_by(|&a, &b| self.energy(a).total_cmp(&self.energy(b)))
            .map_or(low, |offset| low + offset)
    }

    fn energy(&self, offset: usize) -> f32 {
        (0..TAPS)
            .map(|l| self.correlation[offset + SPS * l].norm_sqr())
            .sum()
    }

    fn gather(&mut self, low: usize, best: usize) {
        let offset = best - low;
        for l in 0..TAPS {
            self.taps[l] = self.correlation[offset + SPS * l];
        }
        for (j, symbol) in self.symbols.iter_mut().enumerate() {
            *symbol = self.rotated[best + SPS * j];
        }
        for state in 0..STATES {
            let mut tail = Complex::<f32>::default();
            for l in 1..TAPS {
                tail += self.taps[l] * level(state >> (l - 1) & 1 == 1);
            }
            self.expected[state] = [tail + self.taps[0], tail - self.taps[0]];
        }
    }

    fn noise(&self, training: &Training) -> f32 {
        let mut sum = 0.0;
        let mut count = 0;
        for j in training.at + TAPS - 1..training.at + training.len() {
            let mut model = Complex::<f32>::default();
            for l in 0..TAPS {
                model += self.taps[l] * training.symbol(j - training.at - l);
            }
            sum += (self.symbols[j] - model).norm_sqr();
            count += 1;
        }
        sum / count.max(1) as f32
    }

    fn branch(&self, j: usize, state: usize, bit: usize, noise: f32) -> f32 {
        -(self.symbols[j] - self.expected[state][bit]).norm_sqr() / noise
    }

    fn detect(&mut self, noise: f32, training: &Training, soft: &mut [f32; BURST_SYMBOLS]) {
        self.alpha[0] = [0.0; STATES];
        for j in 0..BURST_SYMBOLS {
            let mut next = [UNREACHABLE; STATES];
            for state in 0..STATES {
                let from = self.alpha[j][state];
                if from == UNREACHABLE {
                    continue;
                }
                for bit in allowed(training.known(j)) {
                    let to = (state << 1 | bit) & STATE_MASK;
                    let metric = from + self.branch(j, state, bit, noise);
                    if metric > next[to] {
                        next[to] = metric;
                    }
                }
            }
            normalise(&mut next);
            self.alpha[j + 1] = next;
        }
        self.beta = [0.0; STATES];
        for j in (0..BURST_SYMBOLS).rev() {
            let mut best = [UNREACHABLE; 2];
            self.beta_next = [UNREACHABLE; STATES];
            for state in 0..STATES {
                for bit in allowed(training.known(j)) {
                    let to = (state << 1 | bit) & STATE_MASK;
                    let gamma = self.branch(j, state, bit, noise) + self.beta[to];
                    let total = self.alpha[j][state] + gamma;
                    if total > best[bit] {
                        best[bit] = total;
                    }
                    if gamma > self.beta_next[state] {
                        self.beta_next[state] = gamma;
                    }
                }
            }
            normalise(&mut self.beta_next);
            self.beta = self.beta_next;
            soft[j] = llr(best);
        }
    }
}

fn level(one: bool) -> f32 {
    if one { -1.0 } else { 1.0 }
}

fn allowed(known: Option<bool>) -> core::ops::RangeInclusive<usize> {
    match known {
        Some(false) => 0..=0,
        Some(true) => 1..=1,
        None => 0..=1,
    }
}

fn normalise(metrics: &mut [f32; STATES]) {
    let peak = metrics.iter().copied().fold(UNREACHABLE, f32::max);
    if peak.is_finite() {
        for metric in metrics {
            *metric -= peak;
        }
    }
}

fn llr([zero, one]: [f32; 2]) -> f32 {
    match (zero.is_finite(), one.is_finite()) {
        (true, true) => (zero - one).clamp(-MAX_LLR, MAX_LLR),
        (true, false) => MAX_LLR,
        (false, true) => -MAX_LLR,
        (false, false) => 0.0,
    }
}

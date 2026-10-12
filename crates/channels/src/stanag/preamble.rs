use std::f32::consts::FRAC_PI_4;
use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_wire::{StanagInterleave, StanagRate};

const SAMPLES_PER_SYMBOL: usize = 2;
const SEGMENT: usize = 32;
const SEGMENT_SAMPLES: usize = SEGMENT * SAMPLES_PER_SYMBOL;
const PATTERNS: [[u8; 8]; 8] = [
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 4, 0, 4, 0, 4, 0, 4],
    [0, 0, 4, 4, 0, 0, 4, 4],
    [0, 4, 4, 0, 0, 4, 4, 0],
    [0, 0, 0, 0, 4, 4, 4, 4],
    [0, 4, 0, 4, 4, 0, 4, 0],
    [0, 0, 4, 4, 4, 4, 0, 0],
    [0, 4, 4, 0, 4, 0, 0, 4],
];
const SCRAMBLE: [u8; SEGMENT] = [
    7, 4, 3, 0, 5, 1, 5, 0, 2, 2, 1, 1, 5, 7, 4, 3, 5, 0, 2, 6, 2, 1, 6, 2, 0, 0, 5, 0, 5, 2, 6, 6,
];
pub(super) const COMMON: [u8; 9] = [0, 1, 3, 0, 1, 3, 1, 2, 0];
const COMMON_PATTERNS: usize = 4;
const DECODED_SEGMENTS: usize = COMMON.len() + 2;
const CHUNK: usize = 8;
const BAUD: f64 = 2_400.0;
const MIN_SCORE: f32 = 0.65;
const MIN_MARGIN: f32 = 2.0;

pub(super) const RATES: [(u8, u8, StanagRate); 14] = [
    (7, 6, rate(4_800, StanagInterleave::Zero, false)),
    (7, 7, rate(2_400, StanagInterleave::Zero, true)),
    (6, 4, rate(2_400, StanagInterleave::Short, false)),
    (4, 4, rate(2_400, StanagInterleave::Long, false)),
    (6, 5, rate(1_200, StanagInterleave::Short, false)),
    (4, 5, rate(1_200, StanagInterleave::Long, false)),
    (6, 6, rate(600, StanagInterleave::Short, false)),
    (4, 6, rate(600, StanagInterleave::Long, false)),
    (6, 7, rate(300, StanagInterleave::Short, false)),
    (4, 7, rate(300, StanagInterleave::Long, false)),
    (7, 4, rate(150, StanagInterleave::Short, false)),
    (5, 4, rate(150, StanagInterleave::Long, false)),
    (7, 5, rate(75, StanagInterleave::Short, false)),
    (5, 5, rate(75, StanagInterleave::Long, false)),
];

const fn rate(bps: u16, interleave: StanagInterleave, voice: bool) -> StanagRate {
    StanagRate {
        bps,
        interleave,
        voice,
    }
}

pub(super) fn tribits(pattern: u8) -> [u8; SEGMENT] {
    std::array::from_fn(|k| {
        (PATTERNS[usize::from(pattern) % PATTERNS.len()][k % 8] + SCRAMBLE[k]) % 8
    })
}

fn segment(pattern: u8) -> [Complex<f32>; SEGMENT] {
    tribits(pattern).map(|tribit| Complex::from_polar(1.0, FRAC_PI_4 * f32::from(tribit)))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Preamble {
    Missing,
    Unreadable,
    Rate { rate: StanagRate, offset_hz: f64 },
}

#[derive(Clone, Copy, Default)]
struct Sync {
    score: f32,
    start: usize,
}

pub(super) struct PreambleSearch {
    references: [[Complex<f32>; SEGMENT]; 8],
    matched: [Vec<f32>; COMMON_PATTERNS],
}

impl PreambleSearch {
    pub fn new(window: usize) -> Self {
        Self {
            references: std::array::from_fn(|pattern| segment(pattern as u8)),
            matched: std::array::from_fn(|_| vec![0.0; window]),
        }
    }

    pub fn search(&mut self, x: &[Complex<f32>]) -> Preamble {
        let span = DECODED_SEGMENTS * SEGMENT_SAMPLES;
        if x.len() <= span || x.len() > self.matched[0].len() {
            return Preamble::Missing;
        }
        let rms = (x.iter().map(|s| s.norm_sqr()).sum::<f32>() / x.len() as f32).sqrt();
        if rms <= f32::MIN_POSITIVE {
            return Preamble::Missing;
        }
        self.match_common(x);
        let sync = self.locate(x.len() - span, rms);
        if sync.score < MIN_SCORE {
            return Preamble::Missing;
        }
        self.read(x, sync.start)
            .map_or(Preamble::Unreadable, |rate| Preamble::Rate {
                rate,
                offset_hz: self.offset_hz(x, sync.start),
            })
    }

    fn match_common(&mut self, x: &[Complex<f32>]) {
        for (pattern, matched) in self.matched.iter_mut().enumerate() {
            let reference = &self.references[pattern];
            for (start, slot) in matched[..x.len() - SEGMENT_SAMPLES].iter_mut().enumerate() {
                *slot = chunks(&x[start..], reference).map(Complex::norm).sum();
            }
        }
    }

    fn locate(&self, positions: usize, rms: f32) -> Sync {
        let full = (COMMON.len() * SEGMENT) as f32 * rms;
        let mut best = Sync::default();
        for start in 0..positions {
            let sum: f32 = COMMON
                .iter()
                .enumerate()
                .map(|(index, &pattern)| {
                    self.matched[usize::from(pattern)][start + index * SEGMENT_SAMPLES]
                })
                .sum();
            let score = sum / full;
            if score > best.score {
                best = Sync { score, start };
            }
        }
        best
    }

    fn read(&self, x: &[Complex<f32>], start: usize) -> Option<StanagRate> {
        let d1 = self.decide(x, start + COMMON.len() * SEGMENT_SAMPLES)?;
        let d2 = self.decide(x, start + (COMMON.len() + 1) * SEGMENT_SAMPLES)?;
        RATES
            .iter()
            .find(|&&(a, b, _)| a == d1 && b == d2)
            .map(|&(_, _, rate)| rate)
    }

    fn decide(&self, x: &[Complex<f32>], at: usize) -> Option<u8> {
        let mut first = (0u8, 0.0f32);
        let mut second = 0.0f32;
        for (pattern, reference) in self.references.iter().enumerate() {
            let level: f32 = chunks(&x[at..], reference).map(Complex::norm).sum();
            if level > first.1 {
                second = first.1;
                first = (pattern as u8, level);
            } else if level > second {
                second = level;
            }
        }
        (first.1 >= MIN_MARGIN * second).then_some(first.0)
    }

    fn offset_hz(&self, x: &[Complex<f32>], start: usize) -> f64 {
        let mut turn = Complex::<f32>::default();
        for (index, &pattern) in COMMON.iter().enumerate() {
            let reference = &self.references[usize::from(pattern)];
            let at = start + index * SEGMENT_SAMPLES;
            let mut previous: Option<Complex<f32>> = None;
            for chunk in chunks(&x[at..], reference) {
                if let Some(previous) = previous {
                    turn += chunk * previous.conj();
                }
                previous = Some(chunk);
            }
        }
        f64::from(turn.arg()) * BAUD / (TAU * CHUNK as f64)
    }
}

fn chunks<'a>(
    samples: &'a [Complex<f32>],
    reference: &'a [Complex<f32>; SEGMENT],
) -> impl Iterator<Item = Complex<f32>> + 'a {
    reference
        .as_chunks::<CHUNK>()
        .0
        .iter()
        .enumerate()
        .map(move |(index, symbols)| {
            samples[index * CHUNK * SAMPLES_PER_SYMBOL..]
                .iter()
                .step_by(SAMPLES_PER_SYMBOL)
                .zip(symbols)
                .map(|(sample, symbol)| sample * symbol.conj())
                .sum()
        })
}

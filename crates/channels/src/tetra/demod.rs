use num_complex::Complex;
use sdrmm_dsp::{Decimator, farrow};
use sdrmm_modem::pulse::{self, Norm};

use super::coding::Soft;

pub(crate) const SYMBOL_RATE: f64 = 18_000.0;
pub(crate) const SPS: usize = 4;
pub(crate) const INPUT_RATE_HZ: f64 = SYMBOL_RATE * SPS as f64;
pub(crate) const RRC_ALPHA: f64 = 0.35;
const RRC_SPAN: usize = 8;

const TIMING_MEMORY_SYMBOLS: f32 = 96.0;
const TIMING_GAIN: f64 = 0.15;
const FREQUENCY_MEMORY_SYMBOLS: f32 = 256.0;
const LEVEL_MEMORY_SYMBOLS: f32 = 64.0;
const SOFT_SCALE: f32 = 48.0;
const SOFT_LIMIT: f32 = 127.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Dibit {
    pub(crate) first: Soft,
    pub(crate) second: Soft,
    pub(crate) product: Complex<f32>,
}

pub(crate) struct Demod {
    matched: Decimator,
    filtered: Vec<Complex<f32>>,
    buffer: Vec<Complex<f32>>,
    consumed: usize,
    sample: usize,
    position: f64,
    timing: Complex<f32>,
    previous: Option<Complex<f32>>,
    rotation: Complex<f32>,
    level: f32,
}

impl Demod {
    pub(crate) fn new() -> Self {
        Self {
            matched: Decimator::new(&receive_filter(), 1),
            filtered: Vec::new(),
            buffer: Vec::new(),
            consumed: 0,
            sample: 0,
            position: 1.0,
            timing: Complex::new(0.0, 0.0),
            previous: None,
            rotation: Complex::new(0.0, 0.0),
            level: 0.0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.matched.reset();
        self.filtered.clear();
        self.buffer.clear();
        self.consumed = 0;
        self.sample = 0;
        self.position = 1.0;
        self.timing = Complex::new(0.0, 0.0);
        self.previous = None;
        self.rotation = Complex::new(0.0, 0.0);
        self.level = 0.0;
    }

    pub(crate) fn frequency_offset_hz(&self) -> f64 {
        f64::from(self.rotation.arg()) / 4.0 * SYMBOL_RATE / std::f64::consts::TAU
    }

    pub(crate) fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<Dibit>) {
        self.filtered.clear();
        self.matched.process(iq, &mut self.filtered);
        for index in 0..self.filtered.len() {
            let x = self.filtered[index];
            self.track_timing(x);
            self.buffer.push(x);
            self.sample += 1;
        }
        self.emit(out);
        let keep_from = (self.position.floor() as usize).saturating_sub(1);
        let drain = keep_from
            .saturating_sub(self.consumed)
            .min(self.buffer.len());
        self.buffer.drain(..drain);
        self.consumed += drain;
    }

    fn track_timing(&mut self, x: Complex<f32>) {
        let power = x.norm_sqr();
        let line = match self.sample % SPS {
            0 => Complex::new(power, 0.0),
            1 => Complex::new(0.0, -power),
            2 => Complex::new(-power, 0.0),
            _ => Complex::new(0.0, power),
        };
        self.timing += (line - self.timing) / (TIMING_MEMORY_SYMBOLS * SPS as f32);
    }

    fn timing_phase(&self) -> Option<f64> {
        (self.timing.norm_sqr() > 0.0).then(|| {
            let turns = -f64::from(self.timing.arg()) / std::f64::consts::TAU;
            (turns * SPS as f64).rem_euclid(SPS as f64)
        })
    }

    fn emit(&mut self, out: &mut Vec<Dibit>) {
        while self.position.floor() as usize + 2 < self.consumed + self.buffer.len() {
            let base = self.position.floor() as usize;
            let local = base - self.consumed;
            if local == 0 {
                self.position += 1.0;
                continue;
            }
            let mu = (self.position - base as f64) as f32;
            let symbol = farrow(&self.buffer[local - 1..local + 3], mu);
            self.position += SPS as f64 + TIMING_GAIN * self.timing_error();
            if let Some(dibit) = self.differential(symbol) {
                out.push(dibit);
            }
        }
    }

    fn timing_error(&self) -> f64 {
        let Some(target) = self.timing_phase() else {
            return 0.0;
        };
        let current = self.position.rem_euclid(SPS as f64);
        (target - current + SPS as f64 / 2.0).rem_euclid(SPS as f64) - SPS as f64 / 2.0
    }

    fn differential(&mut self, symbol: Complex<f32>) -> Option<Dibit> {
        let previous = self.previous.replace(symbol)?;
        let product = symbol * previous.conj();
        let magnitude = product.norm();
        if magnitude <= 0.0 || !magnitude.is_finite() {
            return Some(Dibit::default());
        }
        let unit = product / magnitude;
        let fourth = unit * unit * unit * unit;
        self.rotation += (-fourth - self.rotation) / FREQUENCY_MEMORY_SYMBOLS;
        self.level += (magnitude - self.level) / LEVEL_MEMORY_SYMBOLS;
        let correction = if self.rotation.norm_sqr() > 0.0 {
            Complex::from_polar(1.0, -self.rotation.arg() / 4.0)
        } else {
            Complex::new(1.0, 0.0)
        };
        let corrected = product * correction / self.level.max(f32::MIN_POSITIVE);
        Some(Dibit {
            first: soft(-corrected.im),
            second: soft(-corrected.re),
            product: corrected,
        })
    }
}

fn soft(value: f32) -> Soft {
    (value * SOFT_SCALE).clamp(-SOFT_LIMIT, SOFT_LIMIT) as Soft
}

pub(crate) fn receive_filter() -> Vec<f32> {
    pulse::root_raised_cosine(SPS as f64, RRC_ALPHA, RRC_SPAN, Norm::Energy)
}

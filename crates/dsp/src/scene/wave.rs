use std::f64::consts::{PI, TAU};

use num_complex::Complex;

use super::{
    array::SceneSignal,
    delay::{self, KAISER_BETA, KaiserSinc},
    rng::Rng,
};
use crate::special::sinc;

pub(super) const DELAY_TAPS: usize = 129;
pub(super) const DELAY_CENTRE: i64 = 64;
const NOISE_TAPS: usize = 63;
const WAVE_PRE_ROLL: i64 = 1024;
pub(super) const ECHO_FRACTIONS: usize = 256;

type C64 = Complex<f64>;

#[derive(Clone, Debug)]
pub(super) struct History {
    pub(super) base: i64,
    pub(super) data: Vec<C64>,
}

impl History {
    pub(super) const fn starting_at(base: i64) -> Self {
        Self {
            base,
            data: Vec::new(),
        }
    }

    pub(super) fn end(&self) -> i64 {
        self.base + self.data.len() as i64
    }

    pub(super) fn get(&self, index: i64) -> C64 {
        usize::try_from(index - self.base)
            .ok()
            .and_then(|offset| self.data.get(offset))
            .copied()
            .unwrap_or_default()
    }

    pub(super) fn trim(&mut self, keep_from: i64) {
        let drop = (keep_from - self.base).clamp(0, self.data.len() as i64) as usize;
        if drop > 0 {
            self.data.drain(..drop);
            self.base += drop as i64;
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct DelayPlan {
    pub(super) delay: f64,
    pub(super) whole: i64,
    taps: Option<Vec<f64>>,
}

impl DelayPlan {
    pub(super) fn new(delay: f64) -> Self {
        let whole = delay.floor();
        let fraction = delay - whole;
        let taps = (fraction > 0.0).then(|| fractional_taps(fraction));
        Self {
            delay,
            whole: whole as i64,
            taps,
        }
    }

    pub(super) fn reach(&self, n: i64) -> i64 {
        let reach = n - self.whole;
        if self.taps.is_some() {
            reach + DELAY_CENTRE
        } else {
            reach
        }
    }

    pub(super) fn read(&self, history: &History, n: i64) -> C64 {
        let at = n - self.whole;
        match &self.taps {
            None => history.get(at),
            Some(taps) => taps
                .iter()
                .enumerate()
                .map(|(k, &tap)| history.get(at - k as i64 + DELAY_CENTRE) * tap)
                .sum(),
        }
    }
}

fn fractional_taps(fraction: f64) -> Vec<f64> {
    let centre = DELAY_CENTRE as f64 + fraction;
    let design = KaiserSinc::new(DELAY_CENTRE as f64 + 1.0, KAISER_BETA);
    let mut taps: Vec<f64> = (0..DELAY_TAPS)
        .map(|k| design.tap(k as f64 - centre))
        .collect();
    delay::normalise(&mut taps);
    taps
}

pub(super) fn fraction_table() -> Vec<Vec<f64>> {
    (0..ECHO_FRACTIONS)
        .map(|step| fractional_taps(step as f64 / ECHO_FRACTIONS as f64))
        .collect()
}

#[derive(Clone, Debug)]
pub(super) struct Wave {
    pub(super) signal: SceneSignal,
    phase: f64,
    rng: Rng,
    lowpass: Vec<f64>,
    white: Vec<C64>,
    pub(super) history: History,
}

impl Wave {
    pub(super) fn new(signal: SceneSignal, mut rng: Rng, sample_rate: f64) -> Self {
        let phase = TAU * rng.uniform();
        let lowpass = match signal {
            SceneSignal::Noise { bandwidth_hz, .. } | SceneSignal::NoiseFm { bandwidth_hz, .. }
                if bandwidth_hz < sample_rate =>
            {
                lowpass_taps(bandwidth_hz / (2.0 * sample_rate))
            }
            _ => Vec::new(),
        };
        Self {
            signal,
            phase,
            rng,
            white: vec![C64::new(0.0, 0.0); lowpass.len()],
            lowpass,
            history: History::starting_at(-WAVE_PRE_ROLL),
        }
    }

    pub(super) fn ensure(&mut self, upto: i64, sample_rate: f64) {
        if !self.signal.is_random() {
            return;
        }
        while self.history.end() <= upto {
            let n = self.history.end();
            let value = self.next_random(n, sample_rate);
            self.history.data.push(value);
        }
    }

    fn next_random(&mut self, n: i64, sample_rate: f64) -> C64 {
        let white = self.rng.complex_normal();
        let shaped = if self.lowpass.is_empty() {
            white
        } else {
            self.white.rotate_right(1);
            self.white[0] = white;
            self.white
                .iter()
                .zip(&self.lowpass)
                .map(|(sample, tap)| sample * tap)
                .sum()
        };
        let mix = TAU * self.signal.offset_hz() * n as f64 / sample_rate;
        if let SceneSignal::NoiseFm { deviation_hz, .. } = self.signal {
            let message = shaped.re * std::f64::consts::SQRT_2;
            self.phase = (self.phase + TAU * deviation_hz * message / sample_rate).rem_euclid(TAU);
            return C64::from_polar(1.0, self.phase + mix);
        }
        shaped * C64::from_polar(1.0, mix)
    }

    pub(super) fn value(&self, n: i64, plan: &DelayPlan, sample_rate: f64) -> C64 {
        self.analytic(n, plan.delay, sample_rate)
            .unwrap_or_else(|| plan.read(&self.history, n))
    }

    pub(super) fn at(&self, n: i64, delay: f64, sample_rate: f64, fractions: &[Vec<f64>]) -> C64 {
        if let Some(value) = self.analytic(n, delay, sample_rate) {
            return value;
        }
        let steps = ECHO_FRACTIONS as f64;
        let quantised = (delay * steps).round() / steps;
        let whole = quantised.floor();
        let step = ((quantised - whole) * steps).round() as usize;
        let at = n - whole as i64;
        match fractions.get(step).filter(|_| step > 0) {
            None => self.history.get(at),
            Some(taps) => taps
                .iter()
                .enumerate()
                .map(|(k, &tap)| self.history.get(at - k as i64 + DELAY_CENTRE) * tap)
                .sum(),
        }
    }

    fn analytic(&self, n: i64, delay: f64, sample_rate: f64) -> Option<C64> {
        let t = (n as f64 - delay) / sample_rate;
        match self.signal {
            SceneSignal::Tone { offset_hz } => {
                Some(C64::from_polar(1.0, TAU * offset_hz * t + self.phase))
            }
            SceneSignal::Fm {
                offset_hz,
                deviation_hz,
                rate_hz,
            } => {
                let swing = if rate_hz > 0.0 {
                    deviation_hz / rate_hz * (TAU * rate_hz * t).sin()
                } else {
                    0.0
                };
                Some(C64::from_polar(
                    1.0,
                    TAU * offset_hz * t + swing + self.phase,
                ))
            }
            SceneSignal::Noise { .. } | SceneSignal::NoiseFm { .. } | SceneSignal::Broadband => {
                None
            }
        }
    }
}

fn lowpass_taps(cutoff: f64) -> Vec<f64> {
    let middle = (NOISE_TAPS - 1) as f64 / 2.0;
    let mut taps: Vec<f64> = (0..NOISE_TAPS)
        .map(|k| {
            let x = k as f64 - middle;
            let window = 0.42 + 0.5 * (PI * x / middle).cos() + 0.08 * (TAU * x / middle).cos();
            2.0 * cutoff * sinc(2.0 * cutoff * x) * window
        })
        .collect();
    let energy: f64 = taps.iter().map(|tap| tap * tap).sum();
    let scale = if energy > 0.0 {
        energy.sqrt().recip()
    } else {
        0.0
    };
    for tap in &mut taps {
        *tap *= scale;
    }
    taps
}

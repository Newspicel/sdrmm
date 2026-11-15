use std::f64::consts::{PI, SQRT_2, TAU};

use num_complex::Complex;

use super::array::SceneSignal;
use super::fractional::{DelayPlan, read_quantised};
use super::history::History;
use crate::{math::rng::Rng, math::special::sinc};

const NOISE_TAPS: usize = 63;
const WAVE_PRE_ROLL: i64 = 1024;
const BLACKMAN: [f64; 3] = [0.42, 0.5, 0.08];

type C64 = Complex<f64>;

/// One source signal: tones and FM computed exactly at any delay, noise kept as history.
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
            let message = shaped.re * SQRT_2;
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
        self.analytic(n, delay, sample_rate)
            .unwrap_or_else(|| read_quantised(&self.history, n, delay, fractions))
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
            let [a0, a1, a2] = BLACKMAN;
            let window = a0 + a1 * (PI * x / middle).cos() + a2 * (TAU * x / middle).cos();
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

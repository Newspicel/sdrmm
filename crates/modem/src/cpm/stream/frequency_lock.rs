use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::{Decimator, design_lowpass, fast_arg, math::special::wrap_rad};

use super::steadiness::Steadiness;

const STEADY_SYMBOLS: f32 = 8.0;

const COARSE_SYMBOLS: f64 = 64.0;

const DRIFT_SYMBOLS: f64 = 64.0;

const FAR_CYCLES_PER_SYMBOL: f64 = 0.125;

const BAND_SYMBOLS: f64 = 0.85;

const BAND_TAPS_PER_SYMBOL: usize = 4;

const LEVEL_SYMBOLS: f64 = 4.0;

const LEVEL_LEAK: f64 = 0.125;

const FREQ_LIMIT_CYCLES_PER_SAMPLE: f64 = 0.1;
#[derive(Clone, Debug)]
pub(super) struct FrequencyLock {
    phase: f64,
    freq: f64,
    gain: f64,
    limit: f64,
    previous: Complex<f32>,
    steadiness: Steadiness,
    band: Decimator,
    banded: Vec<Complex<f32>>,
    upper: f64,
    lower: f64,
    level_alpha: f64,
    drift: f64,
    drift_alpha: f64,
    far_limit: f64,
}

impl FrequencyLock {
    pub(super) fn new(sps: usize) -> Self {
        Self {
            phase: 0.0,
            freq: 0.0,
            gain: (COARSE_SYMBOLS * sps as f64).recip(),
            limit: TAU * FREQ_LIMIT_CYCLES_PER_SAMPLE,
            previous: Complex::new(0.0, 0.0),
            steadiness: Steadiness::new(STEADY_SYMBOLS * sps as f32),
            band: Decimator::new(
                &design_lowpass(BAND_TAPS_PER_SYMBOL * sps + 1, BAND_SYMBOLS / sps as f64),
                1,
            ),
            banded: Vec::new(),
            upper: 0.0,
            lower: 0.0,
            level_alpha: (LEVEL_SYMBOLS * sps as f64).recip(),
            drift: 0.0,
            drift_alpha: (DRIFT_SYMBOLS * sps as f64).recip(),
            far_limit: TAU * FAR_CYCLES_PER_SYMBOL / sps as f64,
        }
    }

    pub(super) fn derotate(&mut self, iq: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        out.clear();
        let step = Complex::from_polar(1.0, -self.freq);
        let mut rotor = Complex::from_polar(1.0, -self.phase);
        for &s in iq {
            out.push(s * Complex::new(rotor.re as f32, rotor.im as f32));
            rotor *= step;
        }
        self.phase = wrap_rad(self.phase + self.freq * iq.len() as f64);
    }

    pub(super) fn observe(&mut self, derotated: &[Complex<f32>], hold: bool) {
        let gain = if hold { 0.0 } else { self.gain };
        self.band.process(derotated, &mut self.banded);
        for k in 0..self.banded.len() {
            let y = self.banded[k];
            self.steadiness.push(y.norm_sqr());
            let turn = y * self.previous.conj();
            self.previous = y;
            if self.steadiness.power() <= f32::MIN_POSITIVE {
                continue;
            }
            let weight = f64::from(self.steadiness.weight());
            let error = self.track_levels(f64::from(fast_arg(turn)));
            self.drift += self.drift_alpha * weight * (error - self.drift);
            self.shift(gain * weight * error);
        }
    }

    pub(super) fn far(&self) -> bool {
        self.drift.abs() > self.far_limit
    }

    pub(super) fn shift(&mut self, rad_per_sample: f64) {
        self.freq = (self.freq + rad_per_sample).clamp(-self.limit, self.limit);
    }

    pub(super) fn freq_cycles_per_sample(&self) -> f64 {
        self.freq / TAU
    }

    pub(super) fn reset(&mut self) {
        self.phase = 0.0;
        self.freq = 0.0;
        self.previous = Complex::new(0.0, 0.0);
        self.steadiness.reset();
        self.band.reset();
        self.upper = 0.0;
        self.lower = 0.0;
        self.drift = 0.0;
    }

    fn track_levels(&mut self, turn: f64) -> f64 {
        let centre = f64::midpoint(self.upper, self.lower);
        let (near, far) = if turn >= centre {
            (&mut self.upper, &mut self.lower)
        } else {
            (&mut self.lower, &mut self.upper)
        };
        *near += self.level_alpha * (turn - *near);
        *far += self.level_alpha * LEVEL_LEAK * (turn - *far);
        f64::midpoint(self.upper, self.lower)
    }
}

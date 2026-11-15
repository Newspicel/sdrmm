use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::math::complex::{narrow, widen};

const OFFSET_LOOP_GAIN: f64 = 0.02;

const OFFSET_LIMIT_SYMBOLS: f64 = 1.0;

const STEP_REFRESH: usize = 16;

const COARSE_SYMBOLS: f64 = 1024.0;

#[derive(Clone, Debug)]
pub(super) struct OffsetTracker {
    gain: f64,
    tone: Complex<f64>,
    frequency: f64,
    limit: f64,
    rotor: Complex<f64>,
    step: Complex<f64>,
    since_step: usize,
    previous: [Complex<f64>; 2],
    power: f64,
    alpha: f64,
    lag: Complex<f64>,
    last: Complex<f64>,
    coarse_alpha: f64,
}

impl OffsetTracker {
    /// Tracks a two-tone signal whose tones sit `shift` cycles per symbol either side of centre.
    pub(super) fn new(shift: f64, sps: f64) -> Self {
        Self {
            gain: OFFSET_LOOP_GAIN / sps,
            tone: Complex::from_polar(1.0, -TAU * (shift / sps)),
            frequency: 0.0,
            limit: TAU * (OFFSET_LIMIT_SYMBOLS / sps),
            rotor: Complex::new(1.0, 0.0),
            step: Complex::new(1.0, 0.0),
            since_step: 0,
            previous: [Complex::new(0.0, 0.0); 2],
            power: 0.0,
            alpha: 1.0 / sps,
            lag: Complex::new(0.0, 0.0),
            last: Complex::new(0.0, 0.0),
            coarse_alpha: 1.0 / (COARSE_SYMBOLS * sps),
        }
    }

    pub(super) fn centre(&mut self, sample: Complex<f32>) -> Complex<f32> {
        if !(sample.re.is_finite() && sample.im.is_finite()) {
            return Complex::new(0.0, 0.0);
        }
        let raw = widen(sample);
        self.lag += self.coarse_alpha * (raw * self.last.conj() - self.lag);
        self.last = raw;
        let turned = raw * self.rotor;
        self.rotor *= self.step;
        self.since_step += 1;
        if self.since_step == STEP_REFRESH {
            self.since_step = 0;
            self.step = Complex::from_polar(1.0, -(self.lag.arg() + self.frequency));
            self.rotor /= self.rotor.norm();
        }
        narrow(turned)
    }

    pub(super) fn follow(&mut self, plus: Complex<f64>, minus: Complex<f64>) {
        let (winner, index, reference) = if plus.norm_sqr() >= minus.norm_sqr() {
            (plus, 0, self.tone)
        } else {
            (minus, 1, self.tone.conj())
        };
        let power = winner.norm_sqr();
        self.power += self.alpha * (power - self.power);
        let rotation = winner * self.previous[index].conj() * reference;
        self.previous = [plus, minus];
        if self.power > 0.0 {
            let error = rotation.im / self.power;
            self.frequency = (self.frequency + self.gain * error).clamp(-self.limit, self.limit);
        }
    }
}

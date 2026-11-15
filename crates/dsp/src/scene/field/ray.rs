use num_complex::Complex;

use super::master::{Master, Symbols};
use super::rotor::Rotor;
use crate::math::complex::narrow;
use crate::scene::world::{Emitter, Waveform};

type C32 = Complex<f32>;
type C64 = Complex<f64>;

#[derive(Clone, Copy, Debug)]
enum Kind {
    Tone,
    Fm { beta: f64, rate_hz: f64 },
    Am { depth: f64, rate_hz: f64 },
    Sampled,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Fold {
    pub(super) rate: f64,
    pub(super) period_s: f64,
}

pub(super) struct Ray {
    kind: Kind,
    offset_hz: f64,
    pub(super) scale: f64,
    shift_s: f64,
    pub(super) fold: Option<Fold>,
    gain: C64,
    pub(super) doppler_hz: f64,
    master: Option<Master>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Wave {
    waveform: Waveform,
    offset_hz: f64,
    seed: u64,
}

impl Wave {
    pub(super) fn of(emitter: &Emitter, offset_hz: f64, seed: u64) -> Self {
        Self {
            waveform: emitter.waveform,
            offset_hz,
            seed,
        }
    }

    pub(super) fn ray(self, gain: C64, shift_s: f64) -> Ray {
        let (kind, master) = match self.waveform {
            Waveform::Tone => (Kind::Tone, None),
            Waveform::Fm {
                deviation_hz,
                rate_hz,
            } => (
                Kind::Fm {
                    beta: deviation_hz / rate_hz,
                    rate_hz,
                },
                None,
            ),
            Waveform::Am { depth, rate_hz } => (Kind::Am { depth, rate_hz }, None),
            Waveform::Noise { bandwidth_hz } => (
                Kind::Sampled,
                Some(Master::new(self.seed, bandwidth_hz, Symbols::Gaussian)),
            ),
            Waveform::Bpsk { symbol_rate } => (
                Kind::Sampled,
                Some(Master::new(self.seed, symbol_rate, Symbols::Binary)),
            ),
        };
        Ray {
            kind,
            offset_hz: self.offset_hz,
            scale: 1.0,
            shift_s,
            fold: None,
            gain,
            doppler_hz: 0.0,
            master,
        }
    }
}

impl Ray {
    fn shift_at(&self, arrival_s: f64) -> f64 {
        match self.fold {
            Some(fold) => {
                let cycles = (arrival_s / fold.period_s).floor();
                self.shift_s - fold.rate * cycles * fold.period_s
            }
            None => self.shift_s,
        }
    }

    pub(super) fn add(&mut self, arrival_s: f64, step_s: f64, out: &mut [C32]) {
        let start = self.scale * arrival_s + self.shift_at(arrival_s);
        let step = self.scale * step_s;
        let mut carrier = Rotor::new(self.offset_hz, start, step);
        let mut doppler = Rotor::new(self.doppler_hz, arrival_s, step_s);
        let gain = self.gain;
        match (self.kind, self.master.as_mut()) {
            (Kind::Fm { beta, rate_hz }, _) => {
                let mut modulation = Rotor::new(rate_hz, start, step);
                for slot in out.iter_mut() {
                    let (sin, cos) = (beta * modulation.next().im).sin_cos();
                    let value = carrier.next() * C64::new(cos, sin);
                    *slot += narrow(gain * doppler.next() * value);
                }
            }
            (Kind::Am { depth, rate_hz }, _) => {
                let mut modulation = Rotor::new(rate_hz, start, step);
                for slot in out.iter_mut() {
                    let envelope = 1.0 + depth * modulation.next().im;
                    *slot += narrow(gain * doppler.next() * carrier.next() * envelope);
                }
            }
            (Kind::Sampled, Some(master)) => {
                for (n, slot) in out.iter_mut().enumerate() {
                    let value = carrier.next() * master.at(start + n as f64 * step);
                    *slot += narrow(gain * doppler.next() * value);
                }
            }
            (Kind::Tone | Kind::Sampled, _) => {
                for slot in out.iter_mut() {
                    *slot += narrow(gain * doppler.next() * carrier.next());
                }
            }
        }
    }
}

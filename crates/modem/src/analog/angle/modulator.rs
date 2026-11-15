use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::{RealDecimator, math::complex::narrow};

use super::{AngleKind, AngleParams};
use crate::analog::stage::message_filter;

pub struct AngleMod {
    kind: AngleKind,
    message: RealDecimator,
    phase: f64,
    limited: Vec<f32>,
}

impl AngleMod {
    #[must_use]
    pub fn new(params: &AngleParams) -> Self {
        Self {
            kind: params.kind,
            message: message_filter(params.audio_taps, params.bandwidth),
            phase: 0.0,
            limited: Vec::new(),
        }
    }

    pub fn process(&mut self, audio: &[f32], out: &mut Vec<Complex<f32>>) {
        self.message.process(audio, &mut self.limited);
        out.clear();
        for &a in &self.limited {
            let phase = match self.kind {
                AngleKind::Fm { deviation } => {
                    self.phase = (self.phase + TAU * deviation * f64::from(a)).rem_euclid(TAU);
                    self.phase
                }
                AngleKind::Pm { deviation_rad } => deviation_rad * f64::from(a),
            };
            out.push(narrow(Complex::from_polar(1.0, phase)));
        }
    }
}

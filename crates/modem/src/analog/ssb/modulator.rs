use num_complex::Complex;
use sdrmm_dsp::RealDecimator;

use super::{SsbMethod, SsbParams, weaver::Weaver};
use crate::analog::filter::{Delay, design_hilbert};

/// The Weaver exciter keeps half the band's power; this restores it.
const WEAVER_GAIN: f32 = 2.0;

enum Exciter {
    Hilbert {
        quadrature: RealDecimator,
        inphase: Delay,
        sign: f64,
        i: Vec<f32>,
        q: Vec<f32>,
    },
    Weaver(Weaver),
}

pub struct SsbMod {
    message: RealDecimator,
    exciter: Exciter,
    limited: Vec<f32>,
}

impl SsbMod {
    #[must_use]
    pub fn new(params: &SsbParams) -> Self {
        let sign = params.sideband.sign();
        let exciter = match params.method {
            SsbMethod::Hilbert => {
                let taps = design_hilbert(params.band_taps | 1);
                Exciter::Hilbert {
                    inphase: Delay::new(taps.len() / 2),
                    quadrature: RealDecimator::new(&taps, 1),
                    sign,
                    i: Vec::new(),
                    q: Vec::new(),
                }
            }
            SsbMethod::Weaver => Exciter::Weaver(Weaver::new(params, -1.0, sign)),
        };
        Self {
            message: params.message_filter(),
            exciter,
            limited: Vec::new(),
        }
    }

    pub fn process(&mut self, audio: &[f32], out: &mut Vec<Complex<f32>>) {
        self.message.process(audio, &mut self.limited);
        out.clear();
        match &mut self.exciter {
            Exciter::Hilbert {
                quadrature,
                inphase,
                sign,
                i,
                q,
            } => {
                inphase.process(&self.limited, i);
                quadrature.process(&self.limited, q);
                out.extend(
                    i.iter()
                        .zip(q.iter())
                        .map(|(&i, &q)| Complex::new(i, *sign as f32 * q)),
                );
            }
            Exciter::Weaver(weaver) => {
                weaver.mix_down(&self.limited, |a, oscillator| oscillator * a);
                out.extend(weaver.mix_up().map(|z| z * WEAVER_GAIN));
            }
        }
    }
}

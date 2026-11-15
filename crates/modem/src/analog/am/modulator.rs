use num_complex::Complex;
use sdrmm_dsp::{FirC, RealDecimator};

use super::{AmMode, AmParams};
use crate::analog::{filter::design_vestigial, stage::message_filter};

pub struct AmMod {
    mode: AmMode,
    message: RealDecimator,
    vestigial: Option<FirC>,
    limited: Vec<f32>,
    shaped: Vec<Complex<f32>>,
}

impl AmMod {
    #[must_use]
    pub fn new(params: &AmParams) -> Self {
        Self {
            mode: params.mode,
            message: message_filter(params.audio_taps, params.bandwidth),
            vestigial: params.vestige.map(|vestige| {
                FirC::new(&design_vestigial(
                    params.band_taps,
                    vestige,
                    params.bandwidth,
                ))
            }),
            limited: Vec::new(),
            shaped: Vec::new(),
        }
    }

    pub fn process(&mut self, audio: &[f32], out: &mut Vec<Complex<f32>>) {
        self.message.process(audio, &mut self.limited);
        let depth = self.mode.depth() as f32;
        let carrier = f32::from(u8::from(matches!(self.mode, AmMode::FullCarrier { .. })));
        out.clear();
        out.extend(
            self.limited
                .iter()
                .map(|&a| Complex::new(carrier + depth * a, 0.0)),
        );
        if let Some(filter) = self.vestigial.as_mut() {
            filter.process(out, &mut self.shaped);
            std::mem::swap(out, &mut self.shaped);
        }
    }
}

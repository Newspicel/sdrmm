use num_complex::Complex;
use sdrmm_dsp::RealDecimator;

use super::{SsbParams, weaver::Weaver};
use crate::analog::{filter::Band, stage::audio_or_copy};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SsbDetector {
    Filter,
    Weaver,
}

enum Detector {
    Filter {
        band: Band,
        filtered: Vec<Complex<f32>>,
    },
    Weaver(Weaver),
}

pub struct SsbDemod {
    detector: Detector,
    audio: Option<RealDecimator>,
    detected: Vec<f32>,
}

impl SsbDemod {
    #[must_use]
    pub fn new(params: &SsbParams, detector: SsbDetector, audio_filter: bool) -> Self {
        let detector = match detector {
            SsbDetector::Filter => Detector::Filter {
                band: params.band().build(),
                filtered: Vec::new(),
            },
            SsbDetector::Weaver => {
                Detector::Weaver(Weaver::new(params, -params.sideband.sign(), 1.0))
            }
        };
        Self {
            detector,
            audio: audio_filter.then(|| params.message_filter()),
            detected: Vec::new(),
        }
    }

    pub fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<f32>) {
        self.detected.clear();
        match &mut self.detector {
            Detector::Filter { band, filtered } => {
                band.process(iq, filtered);
                self.detected.extend(filtered.iter().map(|z| z.re));
            }
            Detector::Weaver(weaver) => {
                weaver.mix_down(iq, |z, oscillator| z * oscillator);
                self.detected.extend(weaver.mix_up().map(|z| z.re));
            }
        }
        audio_or_copy(self.audio.as_mut(), &self.detected, out);
    }
}

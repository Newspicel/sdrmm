use num_complex::Complex;
use sdrmm_dsp::{DcBlocker, FmDemod, Pll, RealDecimator, fast_arg};

use super::{AngleKind, AngleParams};
use crate::analog::{
    filter::Band,
    stage::{LOOP_DAMPING, audio_or_copy, band_or_copy, message_filter},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AngleDetector {
    Discriminator,
    Pll { loop_bw: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AngleRx {
    pub detector: AngleDetector,
    pub predetection: bool,
    pub audio_filter: bool,
    pub dc_block: bool,
}

impl AngleRx {
    #[must_use]
    pub fn new(detector: AngleDetector) -> Self {
        Self {
            detector,
            predetection: true,
            audio_filter: true,
            dc_block: false,
        }
    }

    #[must_use]
    pub fn detector_only(detector: AngleDetector) -> Self {
        Self {
            detector,
            predetection: false,
            audio_filter: false,
            dc_block: false,
        }
    }
}

enum Reader {
    Differential(FmDemod),
    Argument { scale: f32 },
    LoopFrequency { pll: Pll, scale: f64 },
    LoopArgument { pll: Pll, scale: f32 },
}

pub struct AngleDemod {
    band: Option<Band>,
    reader: Reader,
    dc: Option<DcBlocker>,
    audio: Option<RealDecimator>,
    filtered: Vec<Complex<f32>>,
    detected: Vec<f32>,
}

impl AngleDemod {
    const PULL_IN: f64 = 1.5;
    /// The phase loop's pull-in range, in loop bandwidths.
    const PM_PULL_IN: f64 = 10.0;

    #[must_use]
    pub fn new(params: &AngleParams, rx: &AngleRx) -> Self {
        let reader = match (rx.detector, params.kind) {
            (AngleDetector::Discriminator, AngleKind::Fm { deviation }) => {
                Reader::Differential(FmDemod::new(1.0, deviation))
            }
            (AngleDetector::Discriminator, AngleKind::Pm { deviation_rad }) => Reader::Argument {
                scale: (1.0 / deviation_rad) as f32,
            },
            (AngleDetector::Pll { loop_bw }, AngleKind::Fm { deviation }) => {
                Reader::LoopFrequency {
                    pll: Pll::new(loop_bw, LOOP_DAMPING, 0.0, deviation * Self::PULL_IN),
                    scale: 1.0 / deviation,
                }
            }
            (AngleDetector::Pll { loop_bw }, AngleKind::Pm { deviation_rad }) => {
                Reader::LoopArgument {
                    pll: Pll::new(loop_bw, LOOP_DAMPING, 0.0, loop_bw * Self::PM_PULL_IN),
                    scale: (1.0 / deviation_rad) as f32,
                }
            }
        };
        Self {
            band: rx.predetection.then(|| params.band().build()),
            reader,
            dc: rx.dc_block.then(DcBlocker::new),
            audio: rx
                .audio_filter
                .then(|| message_filter(params.audio_taps, params.bandwidth)),
            filtered: Vec::new(),
            detected: Vec::new(),
        }
    }

    pub fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<f32>) {
        band_or_copy(self.band.as_mut(), iq, &mut self.filtered);
        self.detected.clear();
        match &mut self.reader {
            Reader::Differential(fm) => fm.process(&self.filtered, &mut self.detected),
            Reader::Argument { scale } => self
                .detected
                .extend(self.filtered.iter().map(|&z| fast_arg(z) * *scale)),
            Reader::LoopFrequency { pll, scale } => {
                self.detected.extend(self.filtered.iter().map(|&z| {
                    let _ = pll.process(z);
                    (pll.increment_norm() * *scale) as f32
                }));
            }
            Reader::LoopArgument { pll, scale } => {
                self.detected.extend(self.filtered.iter().map(|&z| {
                    let reference = pll.process(z);
                    fast_arg(z * reference.conj()) * *scale
                }));
            }
        }
        if let Some(dc) = self.dc.as_mut() {
            dc.process(&mut self.detected);
        }
        audio_or_copy(self.audio.as_mut(), &self.detected, out);
    }

    #[must_use]
    pub fn lock(&self) -> f32 {
        match &self.reader {
            Reader::Differential(_) | Reader::Argument { .. } => 1.0,
            Reader::LoopFrequency { pll, .. } | Reader::LoopArgument { pll, .. } => pll.lock(),
        }
    }
}

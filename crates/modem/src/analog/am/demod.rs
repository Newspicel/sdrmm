use num_complex::Complex;
use sdrmm_dsp::{Costas, DcBlocker, Pll, RealDecimator};

use super::{AmMode, AmParams};
use crate::analog::{
    filter::Band,
    stage::{LOOP_DAMPING, audio_or_copy, band_or_copy, message_filter},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AmDetector {
    Envelope,
    Synchronous { loop_bw: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AmRx {
    pub detector: AmDetector,
    pub predetection: bool,
    pub audio_filter: bool,
    pub dc_block: bool,
}

impl AmRx {
    #[must_use]
    pub fn new(detector: AmDetector) -> Self {
        Self {
            detector,
            predetection: true,
            audio_filter: true,
            dc_block: true,
        }
    }

    #[must_use]
    pub fn detector_only(detector: AmDetector) -> Self {
        Self {
            detector,
            predetection: false,
            audio_filter: false,
            dc_block: false,
        }
    }
}

enum Carrier {
    Envelope,
    Pll(Pll),
    Costas(Costas),
}

pub struct AmDemod {
    band: Option<Band>,
    carrier: Carrier,
    dc: Option<DcBlocker>,
    audio: Option<RealDecimator>,
    filtered: Vec<Complex<f32>>,
    detected: Vec<f32>,
}

impl AmDemod {
    const PULL_IN: f64 = 10.0;

    #[must_use]
    pub fn new(params: &AmParams, rx: &AmRx) -> Self {
        let carrier = match (rx.detector, params.mode) {
            (AmDetector::Envelope, _) => Carrier::Envelope,
            (AmDetector::Synchronous { loop_bw }, AmMode::FullCarrier { .. }) => Carrier::Pll(
                Pll::new(loop_bw, LOOP_DAMPING, 0.0, loop_bw * Self::PULL_IN),
            ),
            (AmDetector::Synchronous { loop_bw }, AmMode::Suppressed) => Carrier::Costas(
                Costas::new(loop_bw, LOOP_DAMPING, 0.0, loop_bw * Self::PULL_IN),
            ),
        };
        Self {
            band: rx.predetection.then(|| params.band().build()),
            carrier,
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
        let input = &self.filtered;
        self.detected.clear();
        match &mut self.carrier {
            Carrier::Envelope => self
                .detected
                .extend(input.iter().map(|z| z.norm_sqr().sqrt())),
            Carrier::Pll(pll) => self.detected.extend(input.iter().map(|&x| {
                let reference = pll.process(x);
                (x * reference.conj()).re
            })),
            Carrier::Costas(costas) => self
                .detected
                .extend(input.iter().map(|&x| costas.process(x).re)),
        }
        if let Some(dc) = self.dc.as_mut() {
            dc.process(&mut self.detected);
        }
        audio_or_copy(self.audio.as_mut(), &self.detected, out);
    }

    #[must_use]
    pub fn lock(&self) -> f32 {
        match &self.carrier {
            Carrier::Envelope => 1.0,
            Carrier::Pll(pll) => pll.lock(),
            Carrier::Costas(costas) => costas.lock(),
        }
    }
}

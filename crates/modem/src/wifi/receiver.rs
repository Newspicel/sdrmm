use num_complex::Complex;
use sdrmm_dsp::IqDcBlocker;

use super::{RATE_HZ, Sink, dsss::Dsss, ofdm::Ofdm};
use crate::spread::PnError;

/// A one-pole mean with a time constant of 4096 samples.
const DC_CORNER_HZ: f64 = 777.0;

pub struct Receiver {
    dc: IqDcBlocker,
    blocked: Vec<Complex<f32>>,
    dsss: Dsss,
    ofdm: Ofdm,
}

impl Receiver {
    pub fn new() -> Result<Self, PnError> {
        Ok(Self {
            dc: IqDcBlocker::new(RATE_HZ, DC_CORNER_HZ),
            blocked: Vec::new(),
            dsss: Dsss::new()?,
            ofdm: Ofdm::new(),
        })
    }

    pub fn reset(&mut self) {
        self.dc.reset();
        self.dsss.reset();
        self.ofdm.reset();
    }

    #[must_use]
    pub fn rejected(&self) -> u32 {
        self.dsss.rejected().saturating_add(self.ofdm.rejected())
    }

    pub fn process(&mut self, iq: &[Complex<f32>], sink: &mut impl Sink) {
        self.blocked.clear();
        self.blocked.extend_from_slice(iq);
        self.dc.process(&mut self.blocked);
        self.dsss.process(&self.blocked, sink);
        self.ofdm.process(&self.blocked, sink);
    }
}

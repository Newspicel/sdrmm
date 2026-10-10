use num_complex::Complex;

use super::{Sink, dsss::Dsss, ofdm::Ofdm};
use crate::spread::PnError;

const DC_ALPHA: f32 = 1.0 / 4_096.0;

pub struct Receiver {
    dc: Complex<f32>,
    blocked: Vec<Complex<f32>>,
    dsss: Dsss,
    ofdm: Ofdm,
}

impl Receiver {
    pub fn new() -> Result<Self, PnError> {
        Ok(Self {
            dc: Complex::new(0.0, 0.0),
            blocked: Vec::new(),
            dsss: Dsss::new()?,
            ofdm: Ofdm::new(),
        })
    }

    pub fn reset(&mut self) {
        self.dc = Complex::new(0.0, 0.0);
        self.dsss.reset();
        self.ofdm.reset();
    }

    #[must_use]
    pub fn rejected(&self) -> u32 {
        self.dsss.rejected().saturating_add(self.ofdm.rejected())
    }

    pub fn process(&mut self, iq: &[Complex<f32>], sink: &mut impl Sink) {
        self.blocked.clear();
        let dc = &mut self.dc;
        self.blocked.extend(iq.iter().map(|&sample| {
            *dc += (sample - *dc) * DC_ALPHA;
            sample - *dc
        }));
        self.dsss.process(&self.blocked, sink);
        self.ofdm.process(&self.blocked, sink);
    }
}

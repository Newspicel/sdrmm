mod align;
mod chirp;
pub(crate) mod code;
mod filter;
mod payload;
mod receiver;
mod ring;
mod soft;
#[cfg(test)]
mod tests;

use std::ops::RangeInclusive;

use num_complex::Complex;
use sdrmm_wire::{LoraCodingRate, LoraImplicitHeader, LoraIntegrity, LoraIq};

use self::{
    filter::{HALF_TAPS, Prototype},
    receiver::{Receiver, Settings},
    ring::Front,
};

const WIDE_SYMBOLS: usize = 16;
const NARROW_SYMBOLS: usize = 4;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Layout {
    pub bandwidth_hz: f64,
    pub spreading_factors: RangeInclusive<u8>,
    pub iq: LoraIq,
    pub implicit_header: Option<LoraImplicitHeader>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Frame {
    pub spreading_factor: u8,
    pub coding_rate: LoraCodingRate,
    pub sync_word: u8,
    pub implicit_header: bool,
    pub low_data_rate: bool,
    pub inverted_iq: bool,
    pub integrity: LoraIntegrity,
    pub fec_corrected: u32,
    pub snr_db: f32,
    pub frequency_error_hz: f32,
    pub payload: Vec<u8>,
}

impl Frame {
    pub(crate) fn readable(&self) -> bool {
        matches!(self.integrity, LoraIntegrity::CrcOk | LoraIntegrity::NoCrc)
    }
}

pub(crate) struct Decoder {
    front: Front,
    receivers: Vec<Receiver>,
    chunk: usize,
}

impl Decoder {
    #[must_use]
    pub(crate) fn new(layout: &Layout, carrier_hz: f64) -> Self {
        let chips = 1usize << layout.spreading_factors.end();
        let chunk = 2 * chips;
        let receivers = layout
            .spreading_factors
            .clone()
            .flat_map(|spreading_factor| {
                [false, true]
                    .into_iter()
                    .filter(|&inverted| layout.iq.listens(inverted))
                    .map(move |inverted| Settings {
                        spreading_factor,
                        bandwidth_hz: layout.bandwidth_hz,
                        carrier_hz,
                        inverted,
                        implicit_header: layout.implicit_header,
                    })
            })
            .map(Receiver::new)
            .collect();
        Self {
            front: Front::new(
                WIDE_SYMBOLS * 2 * chips + 2 * chunk + 4 * HALF_TAPS,
                NARROW_SYMBOLS * chips + chunk,
                &Prototype::new(),
            ),
            receivers,
            chunk,
        }
    }

    pub(crate) fn retune(&mut self, carrier_hz: f64) {
        for receiver in &mut self.receivers {
            receiver.retune(carrier_hz);
        }
    }

    pub(crate) fn process(&mut self, iq: &[Complex<f32>], frames: &mut Vec<Frame>) {
        for piece in iq.chunks(self.chunk) {
            self.front.push(piece);
            for receiver in &mut self.receivers {
                receiver.run(&self.front, frames);
            }
        }
    }
}

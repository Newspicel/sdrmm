pub mod band;
mod coded;
mod receiver;
mod track;
pub mod transmit;

use sdrmm_dsp::crc24_reflected;

use crate::{
    cpm::{CpmParams, Mapping},
    pulse::{Norm, gaussian_freq},
};

pub use self::receiver::Lane;

pub const SYMBOL_RATE_HZ: f64 = 1_000_000.0;
pub const SPS: usize = 4;
pub const RATE_HZ: f64 = SYMBOL_RATE_HZ * SPS as f64;
pub const DEVIATION_HZ: f64 = 250_000.0;
pub const ACCESS_ADDRESS: u32 = 0x8E89_BED6;
pub const HEADER_BYTES: usize = 2;
pub const CRC_BYTES: usize = 3;
pub const MAX_PAYLOAD_BYTES: usize = 255;
pub const MAX_PDU_BYTES: usize = HEADER_BYTES + MAX_PAYLOAD_BYTES;
pub const CHANNEL_SPACING_HZ: f64 = 2_000_000.0;
pub const RF_CHANNELS: u8 = 40;
const BAND_START_HZ: f64 = 2_402_000_000.0;
const ON_GRID_HZ: f64 = 500_000.0;
const CRC_INIT_REFLECTED: u32 = 0xAA_AAAA;
const CRC_POLY_REFLECTED: u32 = 0xDA_6000;
const BT: f64 = 0.5;
const PULSE_SPAN: usize = 3;
const MODULATION_INDEX: f64 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlePhy {
    Le1m,
    CodedS8,
    CodedS2,
}

#[derive(Clone, Copy, Debug)]
pub struct Packet<'a> {
    pub pdu: &'a [u8],
    pub phy: BlePhy,
    pub rf: Option<u8>,
    pub level_dbfs: f32,
    pub sample: u64,
}

pub trait Sink {
    fn packet(&mut self, packet: Packet<'_>);
}

#[must_use]
pub fn gfsk(sps: f64) -> CpmParams {
    CpmParams::from_h(
        Mapping::natural(2),
        MODULATION_INDEX,
        gaussian_freq(sps, BT, PULSE_SPAN, Norm::Area),
        sps,
    )
}

#[must_use]
pub fn rf_channel_hz(rf: u8) -> f64 {
    BAND_START_HZ + f64::from(rf) * CHANNEL_SPACING_HZ
}

#[must_use]
pub fn rf_channel_at(frequency_hz: f64) -> Option<u8> {
    let steps = ((frequency_hz - BAND_START_HZ) / CHANNEL_SPACING_HZ).round();
    let on_grid = (frequency_hz - BAND_START_HZ - steps * CHANNEL_SPACING_HZ).abs() <= ON_GRID_HZ;
    (on_grid && (0.0..f64::from(RF_CHANNELS)).contains(&steps)).then_some(steps as u8)
}

#[must_use]
pub fn channel_index(rf: u8) -> u8 {
    match rf {
        0 => 37,
        12 => 38,
        39 => 39,
        1..=11 => rf - 1,
        _ => rf - 2,
    }
}

pub struct Whitener(u8);

impl Whitener {
    #[must_use]
    pub fn new(channel_index: u8) -> Self {
        Self(channel_index.reverse_bits() | 0x02)
    }

    pub fn bit(&mut self) -> bool {
        let out = self.0 & 0x80 != 0;
        if out {
            self.0 ^= 0x11;
        }
        self.0 <<= 1;
        out
    }

    pub fn byte(&mut self, byte: u8) -> u8 {
        (0..8).fold(byte, |acc, bit| acc ^ (u8::from(self.bit()) << bit))
    }
}

#[must_use]
pub fn crc24(data: &[u8]) -> u32 {
    crc24_reflected(CRC_POLY_REFLECTED, CRC_INIT_REFLECTED, data)
}

#[must_use]
pub fn crc_ok(pdu_and_crc: &[u8]) -> bool {
    let Some(split) = pdu_and_crc.len().checked_sub(CRC_BYTES) else {
        return false;
    };
    let (pdu, crc) = pdu_and_crc.split_at(split);
    let received = u32::from(crc[0]) | u32::from(crc[1]) << 8 | u32::from(crc[2]) << 16;
    crc24(pdu) == received
}

pub(crate) fn access_address_bits() -> impl Iterator<Item = bool> {
    (0..32).map(|bit| ACCESS_ADDRESS >> bit & 1 == 1)
}

#[cfg(test)]
mod tests;

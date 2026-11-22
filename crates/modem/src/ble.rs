pub mod ad;
pub mod band;
mod channel;
mod coded;
mod front;
pub mod pdu;
mod receiver;
mod track;
pub mod transmit;
mod whitener;

use sdrmm_dsp::crc24_reflected;

use crate::{
    cpm::{CpmParams, Mapping},
    pulse::{Norm, gaussian_freq},
};

pub use self::{
    channel::{channel_index, rf_channel_at, rf_channel_hz},
    front::{Front, FrontError},
    receiver::Lane,
    whitener::Whitener,
};

pub const SYMBOL_RATE_HZ: f64 = 1_000_000.0;
pub const SPS: usize = 4;
pub const RATE_HZ: f64 = SYMBOL_RATE_HZ * SPS as f64;
/// Half the passband a one channel front filters to.
pub const CHANNEL_HALF_PASSBAND_HZ: f64 = RATE_HZ * 0.55 / 2.0;
/// The rate that holds every channel of a band window.
pub const BAND_RATE_HZ: f64 = 20_000_000.0;
pub const DEVIATION_HZ: f64 = 250_000.0;
pub const ACCESS_ADDRESS: u32 = 0x8E89_BED6;
pub const HEADER_BYTES: usize = 2;
pub const CRC_BYTES: usize = 3;
pub const MAX_PAYLOAD_BYTES: usize = 255;
pub const MAX_PDU_BYTES: usize = HEADER_BYTES + MAX_PAYLOAD_BYTES;
const ACCESS_ADDRESS_BITS: usize = 32;
const LE1M_PREAMBLE_BITS: usize = 8;
pub const CHANNEL_SPACING_HZ: f64 = 2_000_000.0;
pub const RF_CHANNELS: u8 = 40;
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

impl BlePhy {
    /// How often a packet of this PHY yields a soft decision. Every PHY sends 1 Msym/s, but
    /// S8 spreads each coded bit over a four symbol block and decides once per block.
    #[must_use]
    pub const fn soft_rate_hz(self) -> f64 {
        match self {
            Self::Le1m | Self::CodedS2 => SYMBOL_RATE_HZ,
            Self::CodedS8 => SYMBOL_RATE_HZ / coded::S8_SYMBOLS as f64,
        }
    }
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

    /// The soft decisions a packet was read from, sign the bit, scaled to a mean magnitude of
    /// one, with the carrier offset it was heard at. LE 1M gives its data symbols, the coded PHYs their
    /// coded bits before the Viterbi decoder, at [`BlePhy::soft_rate_hz`].
    fn symbols(&mut self, _soft: &[f32], _phy: BlePhy, _offset_hz: f64) {}
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
    (0..ACCESS_ADDRESS_BITS).map(|bit| ACCESS_ADDRESS >> bit & 1 == 1)
}

fn le1m_preamble() -> impl Iterator<Item = bool> {
    (0..LE1M_PREAMBLE_BITS).map(|bit| bit % 2 == 1)
}

#[cfg(test)]
mod tests;

use num_complex::Complex;
use sdrmm_dsp::crc16_msb_bits;

use super::{Outcome, WifiPhy, reader::SymbolReader};

const TRAINING_SYMBOLS: usize = 16;
pub(crate) const LONG_SFD: u16 = 0xF3A0;
pub(crate) const SHORT_SFD: u16 = 0x05CF;
/// The scrambled sync field ends in ones before a long SFD and in zeros before a short one.
const LONG_SYNC_TAIL: u32 = 0xFF;
const SHORT_SYNC_TAIL: u32 = 0;
const SYNC_TAIL_BITS: u32 = 8;
const SFD_REGISTER_BITS: u32 = 24;
const HEADER_BITS: usize = 48;
const CRC_BITS: usize = 32;
const CRC_POLY: u16 = 0x1021;
const CRC_INIT: u16 = 0xFFFF;
pub(crate) const LENGTH_EXTENSION: u8 = 0x80;
/// The SIGNAL field of each DSSS rate, its bit rate in units of 100 kb/s.
const SIGNALS: [(u8, WifiPhy); 4] = [
    (0x0A, WifiPhy::Dsss1m),
    (0x14, WifiPhy::Dsss2m),
    (0x37, WifiPhy::Cck5m5),
    (0x6E, WifiPhy::Cck11m),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preamble {
    Long,
    Short,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub phy: WifiPhy,
    pub service: u8,
    pub length_us: u16,
    pub bytes: usize,
}

pub(crate) fn find_sfd(
    reader: &mut SymbolReader<'_>,
    max_symbols: usize,
) -> Result<Preamble, Outcome> {
    let mut register = 0u32;
    let mut training = [Complex::new(0.0f32, 0.0); TRAINING_SYMBOLS];
    for slot in &mut training {
        let differential = reader.raw().ok_or(Outcome::Short)?;
        *slot = differential;
        register = shift(register, reader.descrambled(differential.re < 0.0));
    }
    reader.train(&training);
    for _ in TRAINING_SYMBOLS..max_symbols {
        register = shift(register, reader.dbpsk().ok_or(Outcome::Short)?);
        let sfd = (register >> SYNC_TAIL_BITS) as u16;
        let sync = register & ((1 << SYNC_TAIL_BITS) - 1);
        if sfd == LONG_SFD && sync == LONG_SYNC_TAIL {
            return Ok(Preamble::Long);
        }
        if sfd == SHORT_SFD && sync == SHORT_SYNC_TAIL {
            return Ok(Preamble::Short);
        }
    }
    Err(Outcome::Failed)
}

fn shift(register: u32, bit: bool) -> u32 {
    (register >> 1 | u32::from(bit) << (SFD_REGISTER_BITS - 1)) & ((1 << SFD_REGISTER_BITS) - 1)
}

pub(crate) fn header(reader: &mut SymbolReader<'_>, preamble: Preamble) -> Result<Header, Outcome> {
    let mut bits = [false; HEADER_BITS];
    match preamble {
        Preamble::Long => {
            for bit in &mut bits {
                *bit = reader.dbpsk().ok_or(Outcome::Short)?;
            }
        }
        Preamble::Short => {
            for pair in bits.as_chunks_mut::<2>().0 {
                pair.copy_from_slice(&reader.dqpsk().ok_or(Outcome::Short)?);
            }
        }
    }
    let field = |from: usize, len: usize| {
        bits[from..from + len]
            .iter()
            .enumerate()
            .fold(0u32, |acc, (bit, &value)| acc | u32::from(value) << bit)
    };
    if crc16(&bits[..CRC_BITS]) != received_crc(&bits[CRC_BITS..]) {
        return Err(Outcome::Failed);
    }
    let service = field(8, 8) as u8;
    let length_us = field(16, 16) as u16;
    let phy = phy_for_signal(field(0, 8) as u8).ok_or(Outcome::Failed)?;
    Ok(Header {
        phy,
        service,
        length_us,
        bytes: psdu_bytes(phy, length_us, service),
    })
}

#[must_use]
pub fn psdu_bytes(phy: WifiPhy, length_us: u16, service: u8) -> usize {
    let length = usize::from(length_us);
    match phy {
        WifiPhy::Dsss2m => length / 4,
        WifiPhy::Cck5m5 => length * 11 / 16,
        WifiPhy::Cck11m => {
            (length * 11 / 8).saturating_sub(usize::from(service & LENGTH_EXTENSION != 0))
        }
        _ => length / 8,
    }
}

#[must_use]
pub fn crc16(bits: &[bool]) -> u16 {
    !crc16_msb_bits(CRC_POLY, CRC_INIT, bits)
}

/// The SIGNAL field a DSSS rate sends; 1 Mb/s for anything else.
pub(crate) fn signal_code(phy: WifiPhy) -> u8 {
    SIGNALS
        .iter()
        .find(|(_, candidate)| *candidate == phy)
        .map_or(SIGNALS[0].0, |&(code, _)| code)
}

fn phy_for_signal(code: u8) -> Option<WifiPhy> {
    SIGNALS
        .iter()
        .find(|(candidate, _)| *candidate == code)
        .map(|&(_, phy)| phy)
}

fn received_crc(bits: &[bool]) -> u16 {
    bits.iter()
        .fold(0u16, |acc, &bit| acc << 1 | u16::from(bit))
}

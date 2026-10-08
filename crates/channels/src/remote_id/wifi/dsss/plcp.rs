use num_complex::Complex;
use sdrmm_wire::RemoteIdPhy;

use super::{Outcome, SymbolReader};

const TRAINING_SYMBOLS: usize = 16;
const LONG_SFD: u16 = 0xF3A0;
const SHORT_SFD: u16 = 0x05CF;
const HEADER_BITS: usize = 48;
const CRC_POLY: u16 = 0x1021;
const LENGTH_EXTENSION: u8 = 0x80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Preamble {
    Long,
    Short,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Header {
    pub phy: RemoteIdPhy,
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
        let sfd = (register >> 8) as u16;
        let sync = register & 0xFF;
        if sfd == LONG_SFD && sync == 0xFF {
            return Ok(Preamble::Long);
        }
        if sfd == SHORT_SFD && sync == 0 {
            return Ok(Preamble::Short);
        }
    }
    Err(Outcome::Failed)
}

fn shift(register: u32, bit: bool) -> u32 {
    (register >> 1 | u32::from(bit) << 23) & 0xFF_FFFF
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
    if crc16(&bits[..32]) != received_crc(&bits[32..]) {
        return Err(Outcome::Failed);
    }
    let service = field(8, 8) as u8;
    let length_us = field(16, 16) as u16;
    let phy = match field(0, 8) {
        0x0A => RemoteIdPhy::Dsss1m,
        0x14 => RemoteIdPhy::Dsss2m,
        0x37 => RemoteIdPhy::Cck5m5,
        0x6E => RemoteIdPhy::Cck11m,
        _ => return Err(Outcome::Failed),
    };
    Ok(Header {
        phy,
        service,
        length_us,
        bytes: psdu_bytes(phy, length_us, service),
    })
}

pub(crate) fn psdu_bytes(phy: RemoteIdPhy, length_us: u16, service: u8) -> usize {
    let length = usize::from(length_us);
    match phy {
        RemoteIdPhy::Dsss2m => length / 4,
        RemoteIdPhy::Cck5m5 => length * 11 / 16,
        RemoteIdPhy::Cck11m => {
            (length * 11 / 8).saturating_sub(usize::from(service & LENGTH_EXTENSION != 0))
        }
        _ => length / 8,
    }
}

pub(crate) fn crc16(bits: &[bool]) -> u16 {
    !bits.iter().fold(0xFFFFu16, |register, &bit| {
        let feedback = (register >> 15 == 1) ^ bit;
        let shifted = register << 1;
        if feedback {
            shifted ^ CRC_POLY
        } else {
            shifted
        }
    })
}

fn received_crc(bits: &[bool]) -> u16 {
    bits.iter()
        .fold(0u16, |acc, &bit| acc << 1 | u16::from(bit))
}

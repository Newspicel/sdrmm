use num_complex::Complex;

use super::{
    ACCESS_ADDRESS, BlePhy, CRC_BYTES, SYMBOL_RATE_HZ, Whitener,
    coded::{Encoder, PREAMBLE_PERIOD, PREAMBLE_REPEATS, TERM_BITS, ci_bits, s8_pattern},
    crc24, gfsk,
};
use crate::cpm::CpmMod;

const PREAMBLE_BITS: usize = 8;

fn with_crc(pdu: &[u8], channel_index: u8) -> Vec<u8> {
    let crc = crc24(pdu);
    let mut out = pdu.to_vec();
    out.extend_from_slice(&crc.to_le_bytes()[..CRC_BYTES]);
    let mut whitener = Whitener::new(channel_index);
    for byte in &mut out {
        *byte = whitener.byte(*byte);
    }
    out
}

fn lsb_bits(bytes: &[u8]) -> impl Iterator<Item = bool> + '_ {
    bytes
        .iter()
        .flat_map(|&byte| (0..8).map(move |bit| byte >> bit & 1 == 1))
}

fn access_address() -> impl Iterator<Item = bool> {
    (0..32).map(|bit| ACCESS_ADDRESS >> bit & 1 == 1)
}

fn encode_s8(encoder: &mut Encoder, bits: impl Iterator<Item = bool>, out: &mut Vec<bool>) {
    for bit in bits {
        for coded in encoder.push(bit) {
            out.extend(s8_pattern(coded));
        }
    }
}

#[must_use]
pub fn symbols(pdu: &[u8], channel_index: u8, phy: BlePhy) -> Vec<bool> {
    let payload = with_crc(pdu, channel_index);
    let mut out = Vec::new();
    if phy == BlePhy::Le1m {
        out.extend((0..PREAMBLE_BITS).map(|bit| bit % 2 == 1));
        out.extend(access_address());
        out.extend(lsb_bits(&payload));
        return out;
    }
    out.extend(
        PREAMBLE_PERIOD
            .iter()
            .copied()
            .cycle()
            .take(PREAMBLE_PERIOD.len() * PREAMBLE_REPEATS),
    );
    let mut encoder = Encoder::default();
    let first = access_address()
        .chain(ci_bits(phy))
        .chain(std::iter::repeat_n(false, TERM_BITS));
    encode_s8(&mut encoder, first, &mut out);
    let mut encoder = Encoder::default();
    let second = lsb_bits(&payload).chain(std::iter::repeat_n(false, TERM_BITS));
    if phy == BlePhy::CodedS8 {
        encode_s8(&mut encoder, second, &mut out);
    } else {
        for bit in second {
            out.extend(encoder.push(bit));
        }
    }
    out
}

#[must_use]
pub fn modulate(symbols: &[bool], rate_hz: f64) -> Vec<Complex<f32>> {
    let mut modulator = CpmMod::new(gfsk(rate_hz / SYMBOL_RATE_HZ));
    let indices: Vec<u8> = symbols.iter().map(|&symbol| u8::from(symbol)).collect();
    let mut out = Vec::new();
    modulator.modulate(&indices, &mut out);
    modulator.flush(&mut out);
    out
}

#[must_use]
pub fn packet(pdu: &[u8], channel_index: u8, phy: BlePhy, rate_hz: f64) -> Vec<Complex<f32>> {
    modulate(&symbols(pdu, channel_index, phy), rate_hz)
}

//! Horus Binary layer 2, after `horus_l2.c` in horusdemodlib: a `$$` unique word, then the payload
//! bytes and the (23,12) Golay parity of every 12 payload bits, interleaved and scrambled.

use sdrmm_dsp::coding::fec::{crc16_msb, golay23_correct};

pub(crate) const UNIQUE_WORD: u16 = 0x2424;
pub(crate) const UNIQUE_WORD_BITS: usize = 16;
/// Payload sizes in the order horusdemodlib tries them: the v1 frame, then v2 and the v3 sizes.
pub(crate) const PAYLOAD_SIZES: [usize; 6] = [22, 32, 48, 64, 96, 128];
pub(crate) const MAX_PAYLOAD: usize = 128;
pub(crate) const MAX_CODED_BYTES: usize = coded_bytes(MAX_PAYLOAD);

const DATA_BITS: usize = 12;
const PARITY_BITS: usize = 11;
const CRC_POLY: u16 = 0x1021;
const CRC_INIT: u16 = 0xFFFF;
const SCRAMBLER_SEED: u16 = 0x4A80;
/// The interleaver steps horusdemodlib fixes for the v1 and v2 frames, by coded bytes after the
/// unique word. Every other size takes the next prime above its bit count over 1.62.
const FIXED_STEPS: [(usize, usize); 2] = [(43, 337), (63, 389)];
const STEP_DIVISOR: f64 = 1.62;

/// Bytes on the air for `payload` bytes, unique word included.
#[must_use]
pub(crate) const fn coded_bytes(payload: usize) -> usize {
    let data = payload * 8;
    let codewords = data.div_ceil(DATA_BITS);
    (UNIQUE_WORD_BITS + data + codewords * PARITY_BITS).div_ceil(8)
}

pub(crate) const fn coded_bits(payload: usize) -> usize {
    coded_bytes(payload) * 8
}

/// The payload carried by the coded bytes after the unique word, and how many bits Golay fixed.
pub(crate) fn decode(coded: &mut [u8], payload: &mut [u8]) -> u32 {
    scramble(coded);
    deinterleave(coded);
    unpack(coded, payload)
}

/// The CRC-16/CCITT-FALSE horusdemodlib checks.
pub(crate) fn crc(data: &[u8]) -> u16 {
    crc16_msb(CRC_POLY, CRC_INIT, data)
}

/// The additive scrambler, its own inverse, over bits counted from each byte's LSB.
pub(crate) fn scramble(bytes: &mut [u8]) {
    let mut state = SCRAMBLER_SEED;
    for index in 0..bytes.len() * 8 {
        let out = ((state >> 1) ^ state) & 1;
        bytes[index / 8] ^= (out as u8) << (index % 8);
        state = (state >> 1) | (out << 14);
    }
}

fn interleaver_step(bytes: usize) -> usize {
    if let Some(&(_, step)) = FIXED_STEPS.iter().find(|(size, _)| *size == bytes) {
        return step;
    }
    let start = ((bytes * 8) as f64 / STEP_DIVISOR).floor() as usize;
    let mut step = start + 1;
    while !is_prime(step) {
        step += 1;
    }
    step
}

fn is_prime(n: usize) -> bool {
    n >= 2
        && (2..n)
            .take_while(|d| d * d <= n)
            .all(|d| !n.is_multiple_of(d))
}

fn bit(bytes: &[u8], index: usize) -> u8 {
    (bytes[index / 8] >> (index % 8)) & 1
}

/// Undoes the algebraic interleaver: bit `n` came from bit `step * n mod bits`.
fn deinterleave(bytes: &mut [u8]) {
    let mut out = [0u8; MAX_CODED_BYTES];
    let bits = bytes.len() * 8;
    let step = interleaver_step(bytes.len());
    for n in 0..bits {
        let from = (step * n) % bits;
        out[n / 8] |= bit(bytes, from) << (n % 8);
    }
    bytes.copy_from_slice(&out[..bytes.len()]);
}

#[cfg(test)]
pub(crate) fn interleave(bytes: &mut [u8]) {
    let mut out = [0u8; MAX_CODED_BYTES];
    let bits = bytes.len() * 8;
    let step = interleaver_step(bytes.len());
    for n in 0..bits {
        let to = (step * n) % bits;
        out[to / 8] |= bit(bytes, n) << (to % 8);
    }
    bytes.copy_from_slice(&out[..bytes.len()]);
}

fn msb_bit(bytes: &[u8], index: usize) -> u32 {
    u32::from((bytes[index / 8] >> (7 - index % 8)) & 1)
}

/// Golay-corrects each 12-bit group of payload bits with its 11 parity bits. A short last group
/// sits one bit up from the bottom of its 12, as horusdemodlib packs it.
fn unpack(coded: &[u8], payload: &mut [u8]) -> u32 {
    let data_bits = payload.len() * 8;
    let parity = &coded[payload.len()..];
    payload.fill(0);
    let mut fixed = 0;
    let mut written = 0;
    for group in 0..data_bits.div_ceil(DATA_BITS) {
        let first = group * DATA_BITS;
        let width = DATA_BITS.min(data_bits - first);
        let data =
            (first..first + width).fold(0, |word, index| (word << 1) | msb_bit(coded, index));
        let aligned = if width == DATA_BITS { data } else { data << 1 };
        let check = (0..PARITY_BITS).fold(0, |word, k| {
            (word << 1) | msb_bit(parity, group * PARITY_BITS + k)
        });
        let (word, errors) = golay23_correct((aligned << PARITY_BITS) | check);
        fixed += errors;
        let corrected = word >> PARITY_BITS;
        let bits = if width == DATA_BITS {
            corrected
        } else {
            corrected >> 1
        };
        for k in (0..width).rev() {
            if (bits >> k) & 1 == 1 {
                payload[written / 8] |= 0x80 >> (written % 8);
            }
            written += 1;
        }
    }
    fixed
}

/// The coded bytes after the unique word for `payload`: the encoder side, for tests.
#[cfg(test)]
pub(crate) fn encode(payload: &[u8]) -> Vec<u8> {
    use sdrmm_dsp::coding::fec::golay23_encode;
    let data_bits = payload.len() * 8;
    let mut parity = Vec::new();
    for group in 0..data_bits.div_ceil(DATA_BITS) {
        let first = group * DATA_BITS;
        let width = DATA_BITS.min(data_bits - first);
        let data =
            (first..first + width).fold(0, |word, index| (word << 1) | msb_bit(payload, index));
        let aligned = if width == DATA_BITS { data } else { data << 1 };
        let check = golay23_encode(aligned as u16) & ((1 << PARITY_BITS) - 1);
        parity.extend((0..PARITY_BITS).rev().map(|k| (check >> k) & 1 == 1));
    }
    let mut coded = vec![0u8; coded_bytes(payload.len()) - 2];
    coded[..payload.len()].copy_from_slice(payload);
    for (k, &set) in parity.iter().enumerate() {
        if set {
            let index = payload.len() * 8 + k;
            coded[index / 8] |= 0x80 >> (index % 8);
        }
    }
    interleave(&mut coded);
    scramble(&mut coded);
    coded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_lengths_match_horusdemodlib() {
        assert_eq!(coded_bits(22), 360);
        assert_eq!(coded_bits(32), 520);
        assert_eq!(coded_bytes(128), 249);
    }

    #[test]
    fn the_fixed_steps_hold_for_v1_and_v2() {
        assert_eq!(interleaver_step(43), 337);
        assert_eq!(interleaver_step(63), 389);
        assert_eq!(interleaver_step(247), 1223);
    }

    #[test]
    fn the_crc_is_ccitt_false() {
        assert_eq!(crc(b"123456789"), 0x29B1);
    }

    #[test]
    fn every_size_round_trips() {
        for size in PAYLOAD_SIZES {
            let payload: Vec<u8> = (0..size).map(|n| (n * 37 + 11) as u8).collect();
            let mut coded = encode(&payload);
            let mut out = vec![0; size];
            assert_eq!(decode(&mut coded, &mut out), 0, "size {size}");
            assert_eq!(out, payload, "size {size}");
        }
    }

    #[test]
    fn golay_repairs_scattered_bit_errors() {
        let payload: Vec<u8> = (0..22u8).collect();
        let mut coded = encode(&payload);
        for index in [3, 50, 97, 150, 211, 290] {
            coded[index / 8] ^= 1 << (index % 8);
        }
        let mut out = [0; 22];
        assert_eq!(decode(&mut coded, &mut out), 6);
        assert_eq!(out.as_slice(), payload.as_slice());
    }
}

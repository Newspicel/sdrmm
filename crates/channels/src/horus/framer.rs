//! Finds `$` in the demodulated soft bits and tries each payload size behind it, smallest first,
//! as horusdemodlib does, until one passes its CRC.

use sdrmm_dsp::coding::fec::conv::Soft;

use super::layer2::{
    self, MAX_CODED_BITS, MAX_PAYLOAD, PAYLOAD_SIZES, UNIQUE_WORD, UNIQUE_WORD_BITS, coded_bits,
};
use super::model::HorusFrame;
use super::packet;

/// Bits of `$$` that may be wrong and still start a frame.
const UNIQUE_WORD_SLACK: u32 = 1;
const MAX_CANDIDATES: usize = 32;
const KEPT_BITS: usize = 2 * coded_bits(MAX_PAYLOAD);
const HISTORY_BITS: usize = 2 * KEPT_BITS;

#[derive(Clone, Copy, Debug)]
struct Candidate {
    at: u64,
    next_size: usize,
    exact: bool,
}

pub(crate) struct Framer {
    bits: Vec<Soft>,
    base: u64,
    window: u16,
    seen: u64,
    candidates: Vec<Candidate>,
    coded: [Soft; MAX_CODED_BITS],
    payload: [u8; MAX_PAYLOAD],
    rejected: u32,
}

impl Framer {
    pub(crate) fn new() -> Self {
        Self {
            bits: Vec::with_capacity(HISTORY_BITS),
            base: 0,
            window: 0,
            seen: 0,
            candidates: Vec::with_capacity(MAX_CANDIDATES),
            coded: [0; MAX_CODED_BITS],
            payload: [0; MAX_PAYLOAD],
            rejected: 0,
        }
    }

    /// Frames whose `$$` came clean but no payload size passed its CRC behind it.
    pub(crate) fn rejected(&self) -> u32 {
        self.rejected
    }

    /// Takes the next soft bit, positive for a one.
    pub(crate) fn push(&mut self, bit: Soft, frames: &mut Vec<HorusFrame>) {
        if self.bits.len() == HISTORY_BITS {
            self.bits.drain(..HISTORY_BITS - KEPT_BITS);
            self.base += (HISTORY_BITS - KEPT_BITS) as u64;
        }
        self.bits.push(bit);
        self.seen += 1;
        self.window = (self.window << 1) | u16::from(bit > 0);
        if self.seen >= UNIQUE_WORD_BITS as u64 {
            self.spot_unique_word();
        }
        self.try_candidates(frames);
    }

    fn spot_unique_word(&mut self) {
        let errors = (self.window ^ UNIQUE_WORD).count_ones();
        if errors <= UNIQUE_WORD_SLACK && self.candidates.len() < MAX_CANDIDATES {
            self.candidates.push(Candidate {
                at: self.seen - UNIQUE_WORD_BITS as u64,
                next_size: 0,
                exact: errors == 0,
            });
        }
    }

    fn try_candidates(&mut self, frames: &mut Vec<HorusFrame>) {
        let mut index = 0;
        while index < self.candidates.len() {
            let candidate = self.candidates[index];
            let Some(&size) = PAYLOAD_SIZES.get(candidate.next_size) else {
                if candidate.exact {
                    self.rejected = self.rejected.saturating_add(1);
                }
                self.candidates.remove(index);
                continue;
            };
            let end = candidate.at + coded_bits(size) as u64;
            if end > self.seen {
                index += 1;
                continue;
            }
            match self.decode(candidate.at, size) {
                Some(frame) => {
                    frames.push(frame);
                    self.candidates.retain(|other| other.at >= end);
                    index = 0;
                }
                None => self.candidates[index].next_size += 1,
            }
        }
    }

    fn decode(&mut self, at: u64, size: usize) -> Option<HorusFrame> {
        let start = usize::try_from(at.checked_sub(self.base)?).ok()? + UNIQUE_WORD_BITS;
        let bits = coded_bits(size) - UNIQUE_WORD_BITS;
        let coded = &mut self.coded[..bits];
        coded.copy_from_slice(self.bits.get(start..start + bits)?);
        let payload = &mut self.payload[..size];
        let fixed = layer2::decode(coded, payload);
        let mut frame = packet::parse(payload)?;
        frame.errors_corrected = fixed;
        frame.raw = packet::hex(payload);
        Some(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horus::layer2::{crc, encode};
    use sdrmm_dsp::coding::fec::conv::soft;

    fn on_air(payload: &[u8]) -> Vec<Soft> {
        let mut bytes = UNIQUE_WORD.to_be_bytes().to_vec();
        bytes.extend(encode(payload));
        bytes
            .iter()
            .flat_map(|&byte| (0..8).rev().map(move |k| soft(byte >> k & 1 == 1)))
            .collect()
    }

    fn v1(sequence: u16) -> Vec<u8> {
        let mut payload = vec![7];
        payload.extend(sequence.to_le_bytes());
        payload.extend([1, 2, 3]);
        payload.extend([0; 14]);
        payload.extend(crc(&payload).to_le_bytes());
        payload
    }

    fn framed(bits: &[Soft]) -> (Vec<HorusFrame>, u32) {
        let mut framer = Framer::new();
        let mut frames = Vec::new();
        for &bit in bits {
            framer.push(bit, &mut frames);
        }
        (frames, framer.rejected())
    }

    #[test]
    fn finds_back_to_back_frames_after_noise() {
        let mut bits: Vec<Soft> = (0..301u32)
            .map(|n| soft((n * 7919) >> 3 & 1 == 1))
            .collect();
        for sequence in 0..30 {
            bits.extend(on_air(&v1(sequence)));
        }
        let (frames, _) = framed(&bits);
        let sequences: Vec<u16> = frames.iter().map(|frame| frame.sequence).collect();
        assert_eq!(sequences, (0..30).collect::<Vec<_>>());
    }

    #[test]
    fn a_unique_word_with_one_bad_bit_still_starts_a_frame() {
        let mut bits = on_air(&v1(5));
        bits[3] = -bits[3];
        let (frames, _) = framed(&bits);
        assert_eq!(frames.len(), 1);
    }

    #[test]
    fn a_ruined_frame_counts_as_rejected() {
        let mut bits = on_air(&v1(5));
        for bit in bits.iter_mut().skip(40).step_by(3) {
            *bit = -*bit;
        }
        bits.extend(vec![soft(false); coded_bits(MAX_PAYLOAD)]);
        let (frames, rejected) = framed(&bits);
        assert!(frames.is_empty());
        assert_eq!(rejected, 1);
    }
}

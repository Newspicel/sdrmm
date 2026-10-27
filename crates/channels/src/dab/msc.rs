use sdrmm_dsp::{ConvCode, DAB_DISPERSAL, Prbs, Soft, ViterbiK7, pack_msb};

use super::{fic::GENERATORS, protection::Protection};

pub const CU_BITS: usize = 64;
pub const CUS_PER_CIF: usize = 864;
pub const CIF_BITS: usize = CU_BITS * CUS_PER_CIF;
pub const DEPTH: usize = 16;
pub const PADDING_BITS: usize = 8;

const MAP: [usize; DEPTH] = [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fragment {
    Filling,
    Ready,
    WrongSize { expected: usize, got: usize },
}

pub struct TimeDeinterleaver {
    lines: Vec<Vec<Soft>>,
    at: usize,
    filled: usize,
}

impl TimeDeinterleaver {
    #[must_use]
    pub fn new(fragment: usize) -> Self {
        Self {
            lines: vec![vec![0; fragment]; DEPTH],
            at: 0,
            filled: 0,
        }
    }

    #[must_use]
    pub fn ready(&self) -> bool {
        self.filled > DEPTH
    }

    pub fn push(&mut self, fragment: &[Soft], out: &mut Vec<Soft>) -> Fragment {
        let expected = self.lines[0].len();
        if fragment.len() != expected {
            return Fragment::WrongSize {
                expected,
                got: fragment.len(),
            };
        }
        out.clear();
        out.reserve(fragment.len());
        for (index, &value) in fragment.iter().enumerate() {
            let line = (self.at + MAP[index % DEPTH]) % DEPTH;
            out.push(self.lines[line][index]);
            self.lines[self.at][index] = value;
        }
        self.at = (self.at + 1) % DEPTH;
        self.filled = (self.filled + 1).min(DEPTH * 2);
        if self.ready() {
            Fragment::Ready
        } else {
            Fragment::Filling
        }
    }
}

#[cfg(any(test, feature = "synth"))]
pub struct TimeInterleaver {
    lines: Vec<Vec<bool>>,
    at: usize,
}

#[cfg(any(test, feature = "synth"))]
impl TimeInterleaver {
    #[must_use]
    pub fn new(fragment: usize) -> Self {
        Self {
            lines: vec![vec![false; fragment]; DEPTH],
            at: 0,
        }
    }

    pub fn push(&mut self, fragment: &[bool], out: &mut Vec<bool>) {
        out.clear();
        out.reserve(fragment.len());
        self.lines[self.at].copy_from_slice(fragment);
        for index in 0..fragment.len() {
            let delay = MAP[index % DEPTH];
            let line = (self.at + DEPTH - delay) % DEPTH;
            out.push(self.lines[line][index]);
        }
        self.at = (self.at + 1) % DEPTH;
    }
}

pub struct SubChannelDecoder {
    protection: Protection,
    deinterleaver: TimeDeinterleaver,
    viterbi: ViterbiK7,
    aligned: Vec<Soft>,
    mother: Vec<Soft>,
    bits: Vec<bool>,
}

impl SubChannelDecoder {
    #[must_use]
    pub fn new(protection: Protection) -> Self {
        let fragment = protection.coded_bits();
        Self {
            protection,
            deinterleaver: TimeDeinterleaver::new(fragment),
            viterbi: ViterbiK7::new(ConvCode::new(&GENERATORS)),
            aligned: Vec::new(),
            mother: Vec::new(),
            bits: Vec::new(),
        }
    }

    pub fn frame(&mut self, fragment: &[Soft], out: &mut Vec<u8>) -> Fragment {
        let coded = self.protection.coded_bits();
        let Some(fragment) = without_padding(fragment, coded) else {
            return Fragment::WrongSize {
                expected: coded,
                got: fragment.len(),
            };
        };
        let mut aligned = std::mem::take(&mut self.aligned);
        let state = self.deinterleaver.push(fragment, &mut aligned);
        self.aligned = aligned;
        if state != Fragment::Ready {
            return state;
        }
        self.mother.clear();
        self.protection.depuncture(&self.aligned, &mut self.mother);
        self.bits.clear();
        self.viterbi.decode_tailed(&self.mother, &mut self.bits);
        self.bits.truncate(self.protection.frame_bits());
        Prbs::new(DAB_DISPERSAL).apply_bits(&mut self.bits);
        out.clear();
        out.extend_from_slice(&pack_msb(&self.bits));
        Fragment::Ready
    }
}

fn without_padding(fragment: &[Soft], coded: usize) -> Option<&[Soft]> {
    (fragment.len().checked_sub(coded)? <= PADDING_BITS).then(|| &fragment[..coded])
}

#[cfg(any(test, feature = "synth"))]
pub struct SubChannelEncoder {
    protection: Protection,
    interleaver: TimeInterleaver,
    code: ConvCode,
    bits: Vec<bool>,
    coded: Vec<bool>,
    punctured: Vec<bool>,
}

#[cfg(any(test, feature = "synth"))]
impl SubChannelEncoder {
    #[must_use]
    pub fn new(protection: Protection) -> Self {
        let fragment = protection.coded_bits();
        Self {
            protection,
            interleaver: TimeInterleaver::new(fragment),
            code: ConvCode::new(&GENERATORS),
            bits: Vec::new(),
            coded: Vec::new(),
            punctured: Vec::new(),
        }
    }

    pub fn frame(&mut self, payload: &[u8], out: &mut Vec<bool>) {
        self.bits.clear();
        for &byte in payload {
            for shift in (0..8).rev() {
                self.bits.push(byte >> shift & 1 == 1);
            }
        }
        self.bits.resize(self.protection.frame_bits(), false);
        Prbs::new(DAB_DISPERSAL).apply_bits(&mut self.bits);
        self.bits.extend([false; 6]);
        self.coded.clear();
        self.code.encode(&self.bits, &mut self.coded);
        self.punctured.clear();
        self.protection.puncture(&self.coded, &mut self.punctured);
        let punctured = std::mem::take(&mut self.punctured);
        self.interleaver.push(&punctured, out);
        self.punctured = punctured;
    }
}

#[must_use]
pub fn subchannel_range(start_cu: u16, size_cu: u16) -> Option<(usize, usize)> {
    let start = usize::from(start_cu) * CU_BITS;
    let end = start + usize::from(size_cu) * CU_BITS;
    (end <= CIF_BITS).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use sdrmm_dsp::soft;

    use super::*;
    use crate::dab::protection::{Eep, uep_size_cu};

    fn payload(len: usize, seed: u32) -> Vec<u8> {
        let mut state = seed | 1;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect()
    }

    #[test]
    fn the_interleaver_pair_restores_the_bits_after_sixteen_frames() {
        let fragment = 64;
        let mut interleaver = TimeInterleaver::new(fragment);
        let mut deinterleaver = TimeDeinterleaver::new(fragment);
        let frames: Vec<Vec<bool>> = (0..40)
            .map(|frame| {
                (0..fragment)
                    .map(|index| (index + frame) % 3 == 0)
                    .collect()
            })
            .collect();
        let mut restored = Vec::new();
        for (index, frame) in frames.iter().enumerate() {
            let mut sent = Vec::new();
            interleaver.push(frame, &mut sent);
            let softs: Vec<Soft> = sent.iter().copied().map(soft).collect();
            let mut out = Vec::new();
            if deinterleaver.push(&softs, &mut out) == Fragment::Ready && index >= DEPTH {
                restored.push(out.iter().map(|&value| value > 0).collect::<Vec<bool>>());
            }
        }
        assert!(!restored.is_empty());
        for (index, frame) in restored.iter().enumerate() {
            assert_eq!(*frame, frames[index], "frame {index}");
        }
    }

    #[test]
    fn a_subchannel_logical_frame_round_trips() {
        let protection = Protection::eep(64, Eep::A, 3).expect("EEP-A 3 at 64 kbit/s");
        assert_eq!(protection.coded_bits(), 48 * CU_BITS);
        let mut encoder = SubChannelEncoder::new(protection.clone());
        let mut decoder = SubChannelDecoder::new(protection);
        let frames: Vec<Vec<u8>> = (0..30).map(|index| payload(192, 7 + index)).collect();
        let mut decoded = Vec::new();
        for (index, frame) in frames.iter().enumerate() {
            let mut sent = Vec::new();
            encoder.frame(frame, &mut sent);
            let softs: Vec<Soft> = sent.iter().copied().map(soft).collect();
            let mut out = Vec::new();
            if decoder.frame(&softs, &mut out) == Fragment::Ready && index >= DEPTH {
                decoded.push(out);
            }
        }
        assert!(decoded.len() >= 10);
        for (index, frame) in decoded.iter().enumerate() {
            assert_eq!(*frame, frames[index], "logical frame {index}");
        }
    }

    #[test]
    fn a_fragment_of_the_wrong_length_is_rejected_with_both_sizes() {
        let protection = Protection::eep(64, Eep::A, 3).expect("EEP-A 3 at 64 kbit/s");
        let coded = protection.coded_bits();
        let mut decoder = SubChannelDecoder::new(protection);
        let mut out = Vec::new();
        for length in [coded - CU_BITS, coded + CU_BITS, coded + PADDING_BITS + 4] {
            assert_eq!(
                decoder.frame(&vec![0; length], &mut out),
                Fragment::WrongSize {
                    expected: coded,
                    got: length,
                }
            );
        }
        for length in [coded, coded + 4, coded + PADDING_BITS] {
            assert_eq!(decoder.frame(&vec![0; length], &mut out), Fragment::Filling);
        }
    }

    #[test]
    fn the_padding_of_an_unequal_error_subchannel_is_not_decoded() {
        let table_index = 4;
        let protection = Protection::uep(table_index).expect("UEP index 4");
        let capacity = CU_BITS * usize::from(uep_size_cu(table_index).unwrap_or(0));
        let padding = capacity - protection.coded_bits();
        assert!(padding > 0);
        let mut encoder = SubChannelEncoder::new(protection.clone());
        let mut decoder = SubChannelDecoder::new(protection.clone());
        let frames: Vec<Vec<u8>> = (0..30)
            .map(|index| payload(protection.frame_bits() / 8, 11 + index))
            .collect();
        let mut decoded = Vec::new();
        for (index, frame) in frames.iter().enumerate() {
            let mut sent = Vec::new();
            encoder.frame(frame, &mut sent);
            let mut softs: Vec<Soft> = sent.iter().copied().map(soft).collect();
            softs.resize(capacity, 0);
            let mut out = Vec::new();
            if decoder.frame(&softs, &mut out) == Fragment::Ready && index >= DEPTH {
                decoded.push(out);
            }
        }
        assert!(decoded.len() >= 10);
        for (index, frame) in decoded.iter().enumerate() {
            assert_eq!(*frame, frames[index], "logical frame {index}");
        }
    }

    #[test]
    fn a_subchannel_range_stays_inside_the_common_interleaved_frame() {
        assert_eq!(subchannel_range(0, 864), Some((0, CIF_BITS)));
        assert_eq!(subchannel_range(84, 48), Some((5_376, 8_448)));
        assert_eq!(subchannel_range(860, 48), None);
    }
}

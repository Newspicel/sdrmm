use sdrmm_dsp::coding::fec;
use sdrmm_dsp::coding::fec::BchScratch;

use super::ldpc::Frame;

const NORMAL_POLY: u32 = 0x1_002D;
const MEDIUM_POLY: u32 = 0x802D;
const SHORT_POLY: u32 = 0x402B;

/// The outer BCH code of a DVB-S2 frame: GF(2^16), GF(2^15) or GF(2^14) by frame size.
pub struct Bch {
    code: fec::Bch,
    scratch: BchScratch,
    message: usize,
}

impl Bch {
    #[must_use]
    pub fn new(frame: Frame, correct: usize, message: usize) -> Self {
        let (bits, primitive) = match frame {
            Frame::Short => (14, SHORT_POLY),
            Frame::Medium => (15, MEDIUM_POLY),
            Frame::Normal => (16, NORMAL_POLY),
        };
        let code = fec::Bch::new(bits, primitive, correct);
        Self {
            scratch: code.scratch(),
            code,
            message,
        }
    }

    #[cfg(test)]
    #[must_use]
    pub const fn parity(&self) -> usize {
        self.code.parity()
    }

    #[must_use]
    pub const fn message(&self) -> usize {
        self.message
    }

    pub fn encode(&self, message: &[bool], out: &mut Vec<bool>) {
        self.code.encode(message, out);
    }

    /// Corrects `word` in place. Returns the bits flipped, or `None` when it is beyond repair.
    pub fn decode(&mut self, word: &mut [bool]) -> Option<usize> {
        self.code.decode_with_scratch(word, &mut self.scratch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_decodes_repair_different_words() {
        let mut code = Bch::new(Frame::Short, 12, 7_032);
        assert_eq!(code.decode(&mut []), None);
        for seed in 1..=3 {
            let mut word = Vec::new();
            code.encode(&message(code.message(), seed), &mut word);
            let clean = word.clone();
            for index in 0..seed as usize {
                word[index * 997 + 1] ^= true;
            }
            assert_eq!(code.decode(&mut word), Some(seed as usize));
            assert_eq!(word, clean);
            assert_eq!(code.decode(&mut word), Some(0));
        }
    }

    fn message(len: usize, seed: u32) -> Vec<bool> {
        let mut state = seed | 1;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state & 1 == 1
            })
            .collect()
    }

    #[test]
    fn every_documented_parity_length_comes_out_of_the_generator() {
        for (frame, correct, parity) in [
            (Frame::Normal, 12usize, 192usize),
            (Frame::Normal, 10, 160),
            (Frame::Normal, 8, 128),
            (Frame::Medium, 12, 180),
            (Frame::Short, 12, 168),
        ] {
            let code = Bch::new(frame, correct, 1_000);
            assert_eq!(
                code.parity(),
                parity,
                "t={correct} {frame:?} gave {} parity bits",
                code.parity()
            );
        }
    }

    #[test]
    fn a_single_error_is_located() {
        let mut code = Bch::new(Frame::Normal, 12, 32_208);
        let mut word = Vec::new();
        code.encode(&message(code.message(), 21), &mut word);
        let clean = word.clone();
        word[1_234] ^= true;
        assert_eq!(code.decode(&mut word), Some(1));
        assert_eq!(word, clean);
    }

    #[test]
    fn a_clean_codeword_reports_no_errors() {
        let mut code = Bch::new(Frame::Normal, 12, 32_208);
        let mut word = Vec::new();
        code.encode(&message(code.message(), 3), &mut word);
        assert_eq!(word.len(), 32_400);
        assert_eq!(code.decode(&mut word), Some(0));
    }

    #[test]
    fn errors_up_to_the_design_distance_are_repaired() {
        for (frame, correct, message_bits) in [
            (Frame::Normal, 12usize, 32_208usize),
            (Frame::Normal, 8, 57_472),
            (Frame::Medium, 12, 5_660),
            (Frame::Short, 12, 7_032),
        ] {
            let mut code = Bch::new(frame, correct, message_bits);
            let information = message(message_bits, 5);
            let mut word = Vec::new();
            code.encode(&information, &mut word);
            let clean = word.clone();
            for step in 0..correct {
                word[step * 97 + 3] ^= true;
            }
            assert_eq!(code.decode(&mut word), Some(correct), "t={correct}");
            assert_eq!(word, clean);
        }
    }

    #[test]
    fn one_error_beyond_the_design_distance_is_refused() {
        let mut code = Bch::new(Frame::Normal, 8, 57_472);
        let mut word = Vec::new();
        code.encode(&message(code.message(), 9), &mut word);
        for step in 0..9 {
            word[step * 601 + 11] ^= true;
        }
        assert_eq!(code.decode(&mut word), None);
    }
}

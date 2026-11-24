//! MPT 1327 codewords: 48 information bits under a (63,48) cyclic code whose last check bit is
//! inverted, then an even parity bit over all 64.

use sdrmm_dsp::{
    SyndromeDecoder,
    coding::fec::{Acceptance, chase},
};

/// x^15 + x^14 + x^13 + x^11 + x^4 + x^2 + 1.
const GENERATOR: u128 = 0b1110_1000_0001_0101;
const BLOCK_BITS: u32 = 63;
const CHECK_BITS: u32 = 16;
/// The standard corrects one error and still detects four; more correction falses on noise.
const CORRECTABLE: u32 = 1;
const CHASE_BITS: usize = 4;
pub(crate) const CODEWORD_BITS: usize = 64;

/// The control channel codeword synchronisation sequence.
pub(crate) const SYNC: u16 = 0b1100_0100_1101_0111;
/// The traffic and data channel codeword synchronisation sequence.
pub(crate) const SYNT: u16 = 0b0011_1011_0010_1000;
/// The bit reversals that end a preamble.
pub(crate) const PREAMBLE: u16 = 0b1010_1010_1010_1010;

pub(crate) const INFO_BITS: u32 = 48;
const INFO_MASK: u64 = (1 << INFO_BITS) - 1;
const SYSTEM_MASK: u64 = 0x7FFF;

/// A received codeword whose check bits held, after any repair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Codeword {
    pub word: u64,
    pub errors: u32,
}

impl Codeword {
    /// The 48 information bits, bit 1 of the codeword the most significant.
    pub(crate) const fn info(self) -> u64 {
        self.word >> CHECK_BITS
    }

    /// Bit 1 set: the codeword opens a message.
    pub(crate) const fn is_address(self) -> bool {
        self.info() >> (INFO_BITS - 1) & 1 == 1
    }

    /// The system identity a control or data channel system codeword carries, when this is one:
    /// a data codeword ending in a preamble whose check bits spell the channel's sync.
    pub(crate) fn system(self) -> Option<SystemCodeword> {
        if self.is_address() || self.info() as u16 != PREAMBLE {
            return None;
        }
        let control = match self.word as u16 {
            SYNC => true,
            SYNT => false,
            _ => return None,
        };
        Some(SystemCodeword {
            system: (self.info() >> 32 & SYSTEM_MASK) as u16,
            control,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SystemCodeword {
    pub system: u16,
    pub control: bool,
}

pub(crate) struct Code {
    code: SyndromeDecoder,
}

impl Code {
    pub(crate) fn new() -> Self {
        Self {
            code: SyndromeDecoder::new(GENERATOR, BLOCK_BITS, CORRECTABLE),
        }
    }

    pub(crate) fn encode(&self, info: u64) -> u64 {
        let block = self.code.encode(u128::from(info & INFO_MASK)) as u64 ^ 1;
        block << 1 | u64::from(block.count_ones() & 1)
    }

    /// The codeword nearest `word` by Chase search over its weakest bits, `reliability[i]` that
    /// of bit `i`.
    pub(crate) fn decode_soft(
        &self,
        word: u64,
        reliability: &[f32; CODEWORD_BITS],
    ) -> Option<Codeword> {
        let repair = |trial: u64| self.decode(trial).map(|codeword| codeword.word);
        let acceptance = Acceptance::Radius(CORRECTABLE);
        let fixed = chase(word, reliability, CHASE_BITS, acceptance, repair)?;
        Some(Codeword {
            word: fixed,
            errors: (fixed ^ word).count_ones(),
        })
    }

    /// The codeword `word` was sent as, or none when the check bits cannot account for it.
    pub(crate) fn decode(&self, word: u64) -> Option<Codeword> {
        let odd = word.count_ones() & 1 == 1;
        let block = u128::from(word >> 1 ^ 1);
        let (fixed, errors) = self.code.decode(block)?;
        let errors = match (errors, odd) {
            (0, false) => 0,
            (0 | 1, true) => 1,
            _ => return None,
        };
        let block = fixed as u64 ^ 1;
        Some(Codeword {
            word: block << 1 | u64::from(block.count_ones() & 1),
            errors,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALOHA: u64 = 0x8000_0401_B800;
    const BROADCAST: u64 = 0x8A80_4C72_04B4;

    #[test]
    fn an_encoded_codeword_has_even_parity_and_decodes_whole() {
        let code = Code::new();
        for info in [ALOHA, BROADCAST, 0, INFO_MASK] {
            let word = code.encode(info);
            assert_eq!(word.count_ones() % 2, 0);
            assert_eq!(code.decode(word), Some(Codeword { word, errors: 0 }));
        }
    }

    #[test]
    fn one_flipped_bit_anywhere_is_repaired() {
        let code = Code::new();
        let word = code.encode(BROADCAST);
        for bit in 0..64 {
            let heard = code.decode(word ^ 1 << bit).expect("repaired");
            assert_eq!(heard, Codeword { word, errors: 1 }, "bit {bit}");
        }
    }

    #[test]
    fn two_flipped_bits_are_refused() {
        let code = Code::new();
        let word = code.encode(ALOHA);
        for (a, b) in [(0, 1), (5, 40), (17, 63), (30, 31)] {
            assert_eq!(
                code.decode(word ^ 1 << a ^ 1 << b),
                None,
                "bits {a} and {b}"
            );
        }
    }

    #[test]
    fn three_errors_on_weak_bits_are_repaired_softly() {
        let code = Code::new();
        let word = code.encode(BROADCAST);
        let broken = word ^ (1 << 3 | 1 << 30 | 1 << 61);
        assert_eq!(code.decode(broken), None);
        let mut reliability = [1.0; CODEWORD_BITS];
        for bit in [3, 30, 61] {
            reliability[bit] = 0.05;
        }
        assert_eq!(
            code.decode_soft(broken, &reliability),
            Some(Codeword { word, errors: 3 })
        );
    }

    #[test]
    fn the_air_aloha_reads_as_an_address_codeword() {
        let heard = Code::new()
            .decode(Code::new().encode(ALOHA))
            .expect("valid");
        assert!(heard.is_address());
        assert_eq!(heard.info(), ALOHA);
        assert_eq!(heard.system(), None);
    }
}

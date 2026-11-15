mod viterbi;

pub(crate) use self::viterbi::Viterbi;
use super::{ACCESS_ADDRESS_BITS, BlePhy, access_address_bits};

pub(crate) const G0: u8 = 0b1111;
pub(crate) const G1: u8 = 0b1011;
pub(crate) const TERM_BITS: usize = 3;
pub(crate) const CI_BITS: usize = 2;
pub(crate) const S8_SYMBOLS: usize = 4;
pub(crate) const AA_BLOCKS: usize = 2 * ACCESS_ADDRESS_BITS;
pub(crate) const INDICATOR_BLOCKS: usize = (CI_BITS + TERM_BITS) * 2;
pub(crate) const PREAMBLE_PERIOD: [bool; 8] = [false, false, true, true, true, true, false, false];
pub(crate) const PREAMBLE_REPEATS: usize = 10;

#[derive(Clone, Copy, Default)]
pub(crate) struct Encoder(u8);

impl Encoder {
    pub(crate) fn push(&mut self, bit: bool) -> [bool; 2] {
        let register = u8::from(bit) << 3 | self.0;
        self.0 = (self.0 >> 1) | u8::from(bit) << 2;
        [
            (register & G0).count_ones() & 1 == 1,
            (register & G1).count_ones() & 1 == 1,
        ]
    }
}

pub(crate) fn s8_pattern(coded: bool) -> [bool; S8_SYMBOLS] {
    if coded {
        [true, true, false, false]
    } else {
        [false, false, true, true]
    }
}

pub(crate) fn ci_bits(phy: BlePhy) -> [bool; CI_BITS] {
    [phy == BlePhy::CodedS2, false]
}

pub(crate) fn coded_access_address() -> ([bool; AA_BLOCKS], Encoder) {
    let mut encoder = Encoder::default();
    let mut coded = [false; AA_BLOCKS];
    for (pair, bit) in coded
        .as_chunks_mut::<2>()
        .0
        .iter_mut()
        .zip(access_address_bits())
    {
        *pair = encoder.push(bit);
    }
    (coded, encoder)
}

pub(crate) fn indicator(after_access_address: Encoder, phy: BlePhy) -> [bool; INDICATOR_BLOCKS] {
    let mut encoder = after_access_address;
    let mut coded = [false; INDICATOR_BLOCKS];
    let bits = ci_bits(phy)
        .into_iter()
        .chain(std::iter::repeat_n(false, TERM_BITS));
    for (pair, bit) in coded.as_chunks_mut::<2>().0.iter_mut().zip(bits) {
        *pair = encoder.push(bit);
    }
    coded
}

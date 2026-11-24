use super::{
    conv::Soft,
    remainder::remainder,
    soft_ml::{SoftWord, confident, span},
};

const GOLAY23_POLY: u32 = 0xC75;
const GOLAY23_CODE_BITS: u32 = 23;
const GOLAY23_DATA_BITS: u32 = 12;
const GOLAY23_CHECK_BITS: u32 = GOLAY23_CODE_BITS - GOLAY23_DATA_BITS;
const GOLAY23_CODE_MASK: u32 = (1 << GOLAY23_CODE_BITS) - 1;
const GOLAY23_DATA_MASK: u32 = (1 << GOLAY23_DATA_BITS) - 1;

const GOLAY23_ERROR_PATTERNS: [u32; 1 << GOLAY23_CHECK_BITS] = {
    let mut table = [0; 1 << GOLAY23_CHECK_BITS];
    let mut first = 0;
    while first < GOLAY23_CODE_BITS {
        let one = 1 << first;
        table[golay23_remainder(one) as usize] = one;
        let mut second = first + 1;
        while second < GOLAY23_CODE_BITS {
            let two = one | 1 << second;
            table[golay23_remainder(two) as usize] = two;
            let mut third = second + 1;
            while third < GOLAY23_CODE_BITS {
                let three = two | 1 << third;
                table[golay23_remainder(three) as usize] = three;
                third += 1;
            }
            second += 1;
        }
        first += 1;
    }
    table
};

const GOLAY23_ROWS: [u64; GOLAY23_DATA_BITS as usize] = {
    let mut rows = [0; GOLAY23_DATA_BITS as usize];
    let mut bit = 0;
    while bit < rows.len() {
        rows[bit] = golay23_encode(1 << bit) as u64;
        bit += 1;
    }
    rows
};

#[must_use]
pub const fn golay23_encode(data: u16) -> u32 {
    let payload = (data as u32 & GOLAY23_DATA_MASK) << GOLAY23_CHECK_BITS;
    payload | golay23_remainder(payload)
}

#[must_use]
pub const fn golay23_ok(word: u32) -> bool {
    golay23_remainder(word) == 0
}

#[must_use]
pub fn golay23_correct(word: u32) -> (u32, u32) {
    let word = word & GOLAY23_CODE_MASK;
    let error = GOLAY23_ERROR_PATTERNS[golay23_remainder(word) as usize];
    (word ^ error, error.count_ones())
}

/// Soft maximum likelihood over all 4096 codewords, the most significant bit first and
/// positive for a one: the codeword and the decisions it overturns. Like [`golay23_correct`]
/// it always answers.
#[must_use]
pub fn golay23_decode_soft(soft: &[Soft; GOLAY23_CODE_BITS as usize]) -> (u32, u32) {
    let (values, len) = confident(soft);
    let received = SoftWord::new(&values[..len]);
    let (_, codeword) = received.best(span(&GOLAY23_ROWS));
    (codeword as u32, received.overturned(codeword))
}

const fn golay23_remainder(word: u32) -> u32 {
    remainder(word, GOLAY23_POLY, GOLAY23_CODE_BITS, GOLAY23_CHECK_BITS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding::fec::conv::soft;

    fn soften(word: u32) -> [Soft; 23] {
        std::array::from_fn(|i| soft(word >> (22 - i) & 1 == 1))
    }

    #[test]
    fn soft_decoding_of_hard_input_matches_the_hard_decoder() {
        for data in [0, 0xA53, 0xFFF] {
            let word = golay23_encode(data);
            for flips in [0, 1, 1 << 22 | 1 << 9, 0b101_0000_0000_0100_0000] {
                assert_eq!(
                    golay23_decode_soft(&soften(word ^ flips)),
                    golay23_correct(word ^ flips),
                    "data {data:#x} flips {flips:#x}"
                );
            }
        }
    }

    #[test]
    fn soft_decoding_corrects_four_weak_errors_and_fills_erasures() {
        let word = golay23_encode(0x3C9);
        let flips = 1 << 0 | 1 << 6 | 1 << 13 | 1 << 20;
        assert_ne!(golay23_correct(word ^ flips).0, word);
        let mut received = soften(word ^ flips);
        for (i, value) in received.iter_mut().enumerate() {
            if flips >> (22 - i) & 1 == 1 {
                *value /= 4;
            }
        }
        assert_eq!(golay23_decode_soft(&received), (word, 4));
        let mut erased = soften(word);
        erased[..6].fill(0);
        assert_eq!(golay23_decode_soft(&erased), (word, 0));
    }

    #[test]
    fn golay23_matches_the_published_dcs_word() {
        let data = 0b1000_0001_0011;
        let word = golay23_encode(data);
        assert_eq!(format!("{word:023b}"), "10000001001111101100011");
        assert!(golay23_ok(word));
        assert_eq!(word >> GOLAY23_CHECK_BITS, u32::from(data));
    }

    #[test]
    fn golay23_accepts_what_it_encodes_and_nothing_one_bit_away() {
        for data in 0..1u16 << GOLAY23_DATA_BITS {
            let word = golay23_encode(data);
            assert!(golay23_ok(word), "data {data:#05x}");
            for bit in 0..GOLAY23_CODE_BITS {
                assert!(!golay23_ok(word ^ 1 << bit), "data {data:#05x} bit {bit}");
            }
        }
    }

    #[test]
    fn golay23_repairs_every_error_up_to_its_radius() {
        let word = golay23_encode(0xA53);
        assert_eq!(golay23_correct(word), (word, 0));
        for first in 0..GOLAY23_CODE_BITS {
            assert_eq!(golay23_correct(word ^ 1 << first), (word, 1));
            for second in first + 1..GOLAY23_CODE_BITS {
                assert_eq!(golay23_correct(word ^ 1 << first ^ 1 << second), (word, 2));
                for third in second + 1..GOLAY23_CODE_BITS {
                    assert_eq!(
                        golay23_correct(word ^ 1 << first ^ 1 << second ^ 1 << third),
                        (word, 3)
                    );
                }
            }
        }
    }

    #[test]
    fn golay23_lands_every_word_within_three_bits_of_a_codeword() {
        for word in (0..1u32 << GOLAY23_CODE_BITS).step_by(97) {
            let (corrected, errors) = golay23_correct(word);
            assert!(golay23_ok(corrected), "word {word:#08x}");
            assert!(errors <= 3);
            assert_eq!((corrected ^ word).count_ones(), errors);
        }
    }

    #[test]
    fn every_rotation_and_the_complement_of_a_golay_word_is_also_one() {
        let word = golay23_encode(0b1000_0001_0011);
        for k in 0..GOLAY23_CODE_BITS {
            let rotated = (word << k | word >> (GOLAY23_CODE_BITS - k)) & GOLAY23_CODE_MASK;
            assert!(golay23_ok(rotated), "rotation {k}");
            assert!(
                golay23_ok(rotated ^ GOLAY23_CODE_MASK),
                "complement of rotation {k}"
            );
        }
    }
}

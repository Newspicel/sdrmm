use super::remainder::remainder;

const POCSAG_GEN: u32 = 0x769;
const POCSAG_CODE_BITS: u32 = 31;
const POCSAG_DATA_BITS: u32 = 21;
const POCSAG_CHECK_BITS: u32 = POCSAG_CODE_BITS - POCSAG_DATA_BITS;
const POCSAG_BIT_SYNDROMES: [u32; POCSAG_CODE_BITS as usize] =
    bit_syndromes(POCSAG_GEN, POCSAG_CHECK_BITS);

const ERMES_GEN: u32 = 0o15315;
const ERMES_CODE_BITS: u32 = 30;
const ERMES_DATA_BITS: u32 = 18;
const ERMES_CHECK_BITS: u32 = ERMES_CODE_BITS - ERMES_DATA_BITS;
const ERMES_BIT_SYNDROMES: [u32; ERMES_CODE_BITS as usize] =
    bit_syndromes(ERMES_GEN, ERMES_CHECK_BITS);

/// Decodes a POCSAG BCH(31,21) codeword followed by its even parity bit, which lets a third
/// error be detected rather than miscorrected.
#[must_use]
pub fn pocsag_bch_decode(word: u32) -> Option<(u32, u32)> {
    let syndrome = pocsag_syndrome(word >> 1);
    let odd = even_parity_bit(word) == 1;
    if syndrome == 0 {
        return Some(if odd { (word ^ 1, 1) } else { (word, 0) });
    }
    if let Some(bit) = find_single(&POCSAG_BIT_SYNDROMES, syndrome) {
        let fix = (1 << (bit + 1)) | u32::from(!odd);
        return Some((word ^ fix, if odd { 1 } else { 2 }));
    }
    if odd {
        return None;
    }
    let (a, b) = find_pair(&POCSAG_BIT_SYNDROMES, syndrome)?;
    Some((word ^ (1 << (a + 1)) ^ (1 << (b + 1)), 2))
}

#[must_use]
pub fn pocsag_bch_encode(word21: u32) -> u32 {
    assert!(
        (word21 >> POCSAG_DATA_BITS) == 0,
        "pocsag codeword carries 21 leading bits"
    );
    let field = word21 << POCSAG_CHECK_BITS;
    let word = (field | pocsag_syndrome(field)) << 1;
    word | even_parity_bit(word)
}

#[must_use]
pub fn ermes_bch_decode(word: u32) -> Option<(u32, u32)> {
    let word = word & ((1 << ERMES_CODE_BITS) - 1);
    let syndrome = ermes_syndrome(word);
    if syndrome == 0 {
        return Some((word, 0));
    }
    if let Some(bit) = find_single(&ERMES_BIT_SYNDROMES, syndrome) {
        return Some((word ^ (1 << bit), 1));
    }
    let (first, second) = find_pair(&ERMES_BIT_SYNDROMES, syndrome)?;
    Some((word ^ (1 << first) ^ (1 << second), 2))
}

#[must_use]
pub fn ermes_bch_encode(info: u32) -> u32 {
    assert!(
        info >> ERMES_DATA_BITS == 0,
        "ERMES information exceeds 18 bits"
    );
    let field = info << ERMES_CHECK_BITS;
    field | ermes_syndrome(field)
}

const fn pocsag_syndrome(field: u32) -> u32 {
    remainder(field, POCSAG_GEN, POCSAG_CODE_BITS, POCSAG_CHECK_BITS)
}

const fn ermes_syndrome(word: u32) -> u32 {
    remainder(word, ERMES_GEN, ERMES_CODE_BITS, ERMES_CHECK_BITS)
}

const fn bit_syndromes<const N: usize>(generator: u32, check_bits: u32) -> [u32; N] {
    let mut table = [0; N];
    let mut i = 0;
    while i < N {
        table[i] = remainder(1 << i, generator, N as u32, check_bits);
        i += 1;
    }
    table
}

fn find_single(table: &[u32], syndrome: u32) -> Option<usize> {
    table.iter().position(|&value| value == syndrome)
}

fn find_pair(table: &[u32], syndrome: u32) -> Option<(usize, usize)> {
    table.iter().enumerate().find_map(|(first, &single)| {
        find_single(table, syndrome ^ single)
            .filter(|&second| second > first)
            .map(|second| (first, second))
    })
}

fn even_parity_bit(word: u32) -> u32 {
    word.count_ones() & 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pocsag_round_trips_a_codeword() {
        let word21 = 0x0007_ABCD & ((1 << POCSAG_DATA_BITS) - 1);
        let encoded = pocsag_bch_encode(word21);
        assert_eq!(encoded >> 11, word21);
        assert_eq!(pocsag_bch_decode(encoded), Some((encoded, 0)));
    }

    #[test]
    fn pocsag_corrects_every_single_bit_error() {
        let clean = pocsag_bch_encode(0x0015_5555);
        for bit in 0..32 {
            let corrupt = clean ^ (1 << bit);
            assert_eq!(
                pocsag_bch_decode(corrupt),
                Some((clean, 1)),
                "single error at bit {bit}"
            );
        }
    }

    #[test]
    fn pocsag_corrects_every_two_bit_error() {
        let clean = pocsag_bch_encode(0x000A_C3F1);
        for a in 0..32 {
            for b in (a + 1)..32 {
                let corrupt = clean ^ (1 << a) ^ (1 << b);
                assert_eq!(
                    pocsag_bch_decode(corrupt),
                    Some((clean, 2)),
                    "double error at bits {a},{b}"
                );
            }
        }
    }

    #[test]
    fn pocsag_reports_a_four_bit_error_as_uncorrectable() {
        let clean = pocsag_bch_encode(0x000A_C3F1);
        assert_eq!(pocsag_bch_decode(clean ^ 0b1111), None);
    }

    #[test]
    fn pocsag_detects_every_three_bit_error() {
        let clean = pocsag_bch_encode(0x0001_0F0F);
        for a in 0..32 {
            for b in (a + 1)..32 {
                for c in (b + 1)..32 {
                    let corrupt = clean ^ (1 << a) ^ (1 << b) ^ (1 << c);
                    assert_eq!(
                        pocsag_bch_decode(corrupt),
                        None,
                        "triple error at bits {a},{b},{c}"
                    );
                }
            }
        }
    }

    #[test]
    fn pocsag_never_calls_a_damaged_codeword_clean() {
        let clean = pocsag_bch_encode(0x000A_C3F1);
        for a in 0..32 {
            for b in (a + 1)..32 {
                for c in (b + 1)..32 {
                    for d in (c + 1)..32 {
                        let corrupt = clean ^ (1 << a) ^ (1 << b) ^ (1 << c) ^ (1 << d);
                        assert_ne!(
                            pocsag_bch_decode(corrupt),
                            Some((corrupt, 0)),
                            "quad error at bits {a},{b},{c},{d} passed as clean"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn ermes_matches_the_analytic_vectors() {
        assert_eq!(ermes_bch_encode(1), 0o15315);
        assert_eq!(ermes_bch_encode((1 << 18) - 1), 0x3FFF_FC4B);
    }

    #[test]
    fn ermes_corrects_every_one_and_two_bit_error() {
        let clean = ermes_bch_encode(0x2_A5A5);
        for first in 0..30 {
            assert_eq!(ermes_bch_decode(clean ^ (1 << first)), Some((clean, 1)));
            for second in (first + 1)..30 {
                assert_eq!(
                    ermes_bch_decode(clean ^ (1 << first) ^ (1 << second)),
                    Some((clean, 2)),
                    "errors at {first},{second}"
                );
            }
        }
    }
}

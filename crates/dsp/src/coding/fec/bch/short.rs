use super::super::remainder::remainder;

/// A binary cyclic code of `N` bits, at most 32, that corrects up to two bit errors by syndrome
/// lookup. The generator and check width are data, so any short BCH code fits.
#[derive(Clone, Copy, Debug)]
pub struct ShortBch<const N: usize> {
    generator: u32,
    check_bits: u32,
    bit_syndromes: [u32; N],
}

impl<const N: usize> ShortBch<N> {
    #[must_use]
    pub const fn new(generator: u32, check_bits: u32) -> Self {
        assert!(N <= 32 && check_bits < N as u32, "code must fit 32 bits");
        let mut bit_syndromes = [0; N];
        let mut i = 0;
        while i < N {
            bit_syndromes[i] = remainder(1 << i, generator, N as u32, check_bits);
            i += 1;
        }
        Self {
            generator,
            check_bits,
            bit_syndromes,
        }
    }

    #[must_use]
    pub const fn data_bits(&self) -> u32 {
        N as u32 - self.check_bits
    }

    #[must_use]
    pub const fn syndrome(&self, word: u32) -> u32 {
        remainder(word, self.generator, N as u32, self.check_bits)
    }

    #[must_use]
    pub fn encode(&self, info: u32) -> u32 {
        assert!(
            info >> self.data_bits() == 0,
            "information exceeds the code"
        );
        let field = info << self.check_bits;
        field | self.syndrome(field)
    }

    /// Corrects the low `N` bits; returns the codeword and the number of bits flipped.
    #[must_use]
    pub fn decode(&self, word: u32) -> Option<(u32, u32)> {
        let word = word & Self::mask();
        let syndrome = self.syndrome(word);
        if syndrome == 0 {
            return Some((word, 0));
        }
        if let Some(bit) = self.single(syndrome) {
            return Some((word ^ (1 << bit), 1));
        }
        let (first, second) = self.pair(syndrome)?;
        Some((word ^ (1 << first) ^ (1 << second), 2))
    }

    /// Encodes and appends an even parity bit below the codeword.
    #[must_use]
    pub fn encode_extended(&self, info: u32) -> u32 {
        let word = self.encode(info) << 1;
        word | even_parity_bit(word)
    }

    /// Decodes a codeword followed by its even parity bit, which lets a third error be detected
    /// rather than miscorrected.
    #[must_use]
    pub fn decode_extended(&self, word: u32) -> Option<(u32, u32)> {
        let syndrome = self.syndrome(word >> 1);
        let odd = even_parity_bit(word) == 1;
        if syndrome == 0 {
            return Some(if odd { (word ^ 1, 1) } else { (word, 0) });
        }
        if let Some(bit) = self.single(syndrome) {
            let fix = (1 << (bit + 1)) | u32::from(!odd);
            return Some((word ^ fix, if odd { 1 } else { 2 }));
        }
        if odd {
            return None;
        }
        let (a, b) = self.pair(syndrome)?;
        Some((word ^ (1 << (a + 1)) ^ (1 << (b + 1)), 2))
    }

    const fn mask() -> u32 {
        u32::MAX >> (32 - N as u32)
    }

    fn single(&self, syndrome: u32) -> Option<usize> {
        self.bit_syndromes
            .iter()
            .position(|&value| value == syndrome)
    }

    fn pair(&self, syndrome: u32) -> Option<(usize, usize)> {
        self.bit_syndromes
            .iter()
            .enumerate()
            .find_map(|(first, &single)| {
                self.single(syndrome ^ single)
                    .filter(|&second| second > first)
                    .map(|second| (first, second))
            })
    }
}

fn even_parity_bit(word: u32) -> u32 {
    word.count_ones() & 1
}

#[cfg(test)]
mod tests {
    use super::*;

    const BCH_31_21: ShortBch<31> = ShortBch::new(0x769, 10);
    const BCH_30_18: ShortBch<30> = ShortBch::new(0o15315, 12);

    #[test]
    fn an_extended_codeword_round_trips() {
        let word21 = 0x0007_ABCD & 0x1F_FFFF;
        let encoded = BCH_31_21.encode_extended(word21);
        assert_eq!(encoded >> 11, word21);
        assert_eq!(BCH_31_21.decode_extended(encoded), Some((encoded, 0)));
    }

    #[test]
    fn the_extended_code_corrects_every_single_bit_error() {
        let clean = BCH_31_21.encode_extended(0x0015_5555);
        for bit in 0..32 {
            let corrupt = clean ^ (1 << bit);
            assert_eq!(
                BCH_31_21.decode_extended(corrupt),
                Some((clean, 1)),
                "single error at bit {bit}"
            );
        }
    }

    #[test]
    fn the_extended_code_corrects_every_two_bit_error() {
        let clean = BCH_31_21.encode_extended(0x000A_C3F1);
        for a in 0..32 {
            for b in (a + 1)..32 {
                let corrupt = clean ^ (1 << a) ^ (1 << b);
                assert_eq!(
                    BCH_31_21.decode_extended(corrupt),
                    Some((clean, 2)),
                    "double error at bits {a},{b}"
                );
            }
        }
    }

    #[test]
    fn the_extended_code_reports_a_four_bit_error_as_uncorrectable() {
        let clean = BCH_31_21.encode_extended(0x000A_C3F1);
        assert_eq!(BCH_31_21.decode_extended(clean ^ 0b1111), None);
    }

    #[test]
    fn the_extended_code_detects_every_three_bit_error() {
        let clean = BCH_31_21.encode_extended(0x0001_0F0F);
        for a in 0..32 {
            for b in (a + 1)..32 {
                for c in (b + 1)..32 {
                    let corrupt = clean ^ (1 << a) ^ (1 << b) ^ (1 << c);
                    assert_eq!(
                        BCH_31_21.decode_extended(corrupt),
                        None,
                        "triple error at bits {a},{b},{c}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_extended_code_never_calls_a_damaged_codeword_clean() {
        let clean = BCH_31_21.encode_extended(0x000A_C3F1);
        for a in 0..32 {
            for b in (a + 1)..32 {
                for c in (b + 1)..32 {
                    for d in (c + 1)..32 {
                        let corrupt = clean ^ (1 << a) ^ (1 << b) ^ (1 << c) ^ (1 << d);
                        assert_ne!(
                            BCH_31_21.decode_extended(corrupt),
                            Some((corrupt, 0)),
                            "quad error at bits {a},{b},{c},{d} passed as clean"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn encoding_matches_the_analytic_vectors() {
        assert_eq!(BCH_30_18.encode(1), 0o15315);
        assert_eq!(BCH_30_18.encode((1 << 18) - 1), 0x3FFF_FC4B);
    }

    #[test]
    fn every_one_and_two_bit_error_is_corrected() {
        let clean = BCH_30_18.encode(0x2_A5A5);
        for first in 0..30 {
            assert_eq!(BCH_30_18.decode(clean ^ (1 << first)), Some((clean, 1)));
            for second in (first + 1)..30 {
                assert_eq!(
                    BCH_30_18.decode(clean ^ (1 << first) ^ (1 << second)),
                    Some((clean, 2)),
                    "errors at {first},{second}"
                );
            }
        }
    }

    #[test]
    fn bits_above_the_code_are_ignored() {
        let clean = BCH_30_18.encode(0x1_2345);
        assert_eq!(BCH_30_18.decode(clean | 0xC000_0000), Some((clean, 0)));
    }
}

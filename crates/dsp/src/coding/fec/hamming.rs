//! The nibble codes cut from the extended Hamming (8,4) code.
//!
//! `parity_bits` from 4 down to 2 keeps that many of the parity bits p0 p1 p2 p3, giving the
//! (8,4) code, the (7,4) Hamming code and a (6,4) code; 1 is a single even parity bit. A
//! codeword is `4 + parity_bits` bits, sent highest first: d0 d1 d2 d3, then the parity.

#[must_use]
pub const fn codeword_len(parity_bits: u8) -> usize {
    4 + parity_bits as usize
}

const fn bit(value: u8, index: u8) -> u8 {
    (value >> index) & 1
}

/// The codeword of the low four bits of `nibble`.
#[must_use]
pub const fn encode_nibble(nibble: u8, parity_bits: u8) -> u8 {
    let (d0, d1, d2, d3) = (
        bit(nibble, 0),
        bit(nibble, 1),
        bit(nibble, 2),
        bit(nibble, 3),
    );
    let data = (d0 << 3) | (d1 << 2) | (d2 << 1) | d3;
    if parity_bits == 1 {
        return (data << 1) | (d0 ^ d1 ^ d2 ^ d3);
    }
    let p0 = d0 ^ d1 ^ d2;
    let p1 = d1 ^ d2 ^ d3;
    let p2 = d0 ^ d1 ^ d3;
    let p3 = d0 ^ d2 ^ d3;
    let full = (data << 4) | (p0 << 3) | (p1 << 2) | (p2 << 1) | p3;
    full >> (4 - parity_bits)
}

/// Bit `index` of a `len`-bit codeword, counting from the first sent.
#[must_use]
pub const fn codeword_bit(codeword: u8, len: usize, index: usize) -> bool {
    (codeword >> (len - 1 - index)) & 1 == 1
}

/// A maximum-likelihood decision over all 16 codewords.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NibbleDecision {
    pub nibble: u8,
    /// The winning codeword differs from the hard decisions.
    pub corrected: bool,
}

/// Decodes one codeword from its log-likelihood ratios, positive for a 1, first sent first.
#[must_use]
pub fn decode_nibble_soft(llrs: &[f32], parity_bits: u8) -> NibbleDecision {
    let len = codeword_len(parity_bits);
    let mut best = (0u8, f32::MIN);
    for nibble in 0..16u8 {
        let codeword = encode_nibble(nibble, parity_bits);
        let score: f32 = (0..len)
            .map(|i| {
                if codeword_bit(codeword, len, i) {
                    llrs[i]
                } else {
                    -llrs[i]
                }
            })
            .sum();
        if score > best.1 {
            best = (nibble, score);
        }
    }
    let codeword = encode_nibble(best.0, parity_bits);
    let corrected = (0..len).any(|i| codeword_bit(codeword, len, i) != (llrs[i] > 0.0));
    NibbleDecision {
        nibble: best.0,
        corrected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn llrs(codeword: u8, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| {
                if codeword_bit(codeword, len, i) {
                    2.0
                } else {
                    -2.0
                }
            })
            .collect()
    }

    #[test]
    fn codewords_have_their_documented_shape() {
        assert_eq!(encode_nibble(0b0001, 4), 0b1000_1011);
        assert_eq!(encode_nibble(0b1111, 4), 0xff);
        assert_eq!(encode_nibble(0b0001, 1), 0b10001);
        for nibble in 0..16 {
            for parity in 1..=4 {
                let codeword = encode_nibble(nibble, parity);
                assert!(u32::from(codeword) < 1 << codeword_len(parity));
                let data = (0..4).fold(0, |acc, i| {
                    acc | (u8::from(codeword_bit(codeword, codeword_len(parity), i)) << i)
                });
                assert_eq!(data, nibble);
            }
        }
    }

    #[test]
    fn the_8_4_code_has_distance_four_and_the_7_4_code_three() {
        for (parity, distance) in [(4, 4), (3, 3), (1, 2)] {
            let nearest = (0..16u8)
                .flat_map(|a| (0..a).map(move |b| (a, b)))
                .map(|(a, b)| (encode_nibble(a, parity) ^ encode_nibble(b, parity)).count_ones())
                .min();
            assert_eq!(nearest, Some(distance), "{parity}");
        }
    }

    #[test]
    fn a_single_weak_error_is_corrected() {
        let codeword = encode_nibble(0b1011, 3);
        let mut received = llrs(codeword, 7);
        received[2] = -received[2] * 0.5;
        let decision = decode_nibble_soft(&received, 3);
        assert_eq!(
            decision,
            NibbleDecision {
                nibble: 0b1011,
                corrected: true
            }
        );
        let clean = decode_nibble_soft(&llrs(codeword, 7), 3);
        assert_eq!(
            clean,
            NibbleDecision {
                nibble: 0b1011,
                corrected: false
            }
        );
    }
}

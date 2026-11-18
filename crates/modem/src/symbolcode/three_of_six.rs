//! The 3-of-6 code of EN 13757-4 mode T: each nibble is a six chip word with three ones.

const CODES: [u8; 16] = [
    0x16, 0x0D, 0x0E, 0x0B, 0x1C, 0x19, 0x1A, 0x13, 0x2C, 0x25, 0x26, 0x23, 0x34, 0x31, 0x32, 0x29,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nibble {
    pub value: u8,
    /// The hard chips were not a codeword and the nearest one by soft score was taken.
    pub repaired: bool,
}

/// The nibble whose codeword best matches six soft chips, positive for a one.
#[must_use]
pub fn three_of_six_decode(soft: &[f32]) -> Nibble {
    let hard = soft
        .iter()
        .fold(0u8, |word, &chip| (word << 1) | u8::from(chip > 0.0));
    let mut best = (0u8, f32::NEG_INFINITY);
    for (value, &code) in (0u8..).zip(CODES.iter()) {
        let score: f32 = soft
            .iter()
            .enumerate()
            .map(|(k, &chip)| {
                if code >> (5 - k) & 1 == 1 {
                    chip
                } else {
                    -chip
                }
            })
            .sum();
        if score > best.1 {
            best = (value, score);
        }
    }
    Nibble {
        value: best.0,
        repaired: CODES[usize::from(best.0)] != hard,
    }
}

/// The two six chip words of a byte, high nibble first.
#[must_use]
pub fn three_of_six_encode(byte: u8) -> [u8; 2] {
    [
        CODES[usize::from(byte >> 4)],
        CODES[usize::from(byte & 0x0F)],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn soft(word: u8) -> Vec<f32> {
        (0..6)
            .rev()
            .map(|k| if word >> k & 1 == 1 { 1.0 } else { -1.0 })
            .collect()
    }

    #[test]
    fn every_codeword_has_three_ones() {
        assert!(CODES.iter().all(|code| code.count_ones() == 3));
    }

    #[test]
    fn every_nibble_survives_three_of_six_and_one_flipped_chip() {
        for byte in 0..=255u8 {
            let [high, low] = three_of_six_encode(byte);
            let mut chips = soft(high);
            chips.extend(soft(low));
            assert_eq!(three_of_six_decode(&chips[..6]).value, byte >> 4);
            assert_eq!(three_of_six_decode(&chips[6..]).value, byte & 0x0F);
            assert!(!three_of_six_decode(&chips[6..]).repaired);
            chips[2] *= -0.2;
            let nibble = three_of_six_decode(&chips[..6]);
            assert_eq!(nibble.value, byte >> 4);
            assert!(nibble.repaired);
        }
    }
}

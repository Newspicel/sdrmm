const CODES: [u8; 16] = [
    0x16, 0x0D, 0x0E, 0x0B, 0x1C, 0x19, 0x1A, 0x13, 0x2C, 0x25, 0x26, 0x23, 0x34, 0x31, 0x32, 0x29,
];
const CRC_POLY: u16 = 0x3D65;

pub(super) struct Nibble {
    pub(super) value: u8,
    pub(super) repaired: bool,
}

pub(super) fn three_of_six(soft: &[f32]) -> Nibble {
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

#[cfg(any(test, feature = "synth"))]
pub(crate) fn encode_three_of_six(byte: u8) -> [u8; 2] {
    [
        CODES[usize::from(byte >> 4)],
        CODES[usize::from(byte & 0x0F)],
    ]
}

pub(super) fn nrz(soft: &[f32]) -> u8 {
    soft.iter()
        .fold(0u8, |byte, &chip| (byte << 1) | u8::from(chip > 0.0))
}

pub(super) fn manchester(soft: &[f32]) -> u8 {
    soft.as_chunks::<2>()
        .0
        .iter()
        .fold(0u8, |byte, &[first, second]| {
            (byte << 1) | u8::from(second > first)
        })
}

pub(crate) fn crc(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ CRC_POLY
            } else {
                crc << 1
            };
        }
    }
    !crc
}

pub(super) fn crc_holds(bytes: &[u8], at: usize) -> bool {
    match (bytes.get(..at), bytes.get(at..at + 2)) {
        (Some(body), Some(&[high, low])) => crc(body) == u16::from_be_bytes([high, low]),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn soft(word: u16, bits: u32) -> Vec<f32> {
        (0..bits)
            .rev()
            .map(|k| if word >> k & 1 == 1 { 1.0 } else { -1.0 })
            .collect()
    }

    #[test]
    fn every_nibble_survives_three_of_six_and_one_flipped_chip() {
        for byte in 0..=255u8 {
            let [high, low] = encode_three_of_six(byte);
            let mut chips = soft(high.into(), 6);
            chips.extend(soft(low.into(), 6));
            assert_eq!(three_of_six(&chips[..6]).value, byte >> 4);
            assert!(!three_of_six(&chips[6..]).repaired);
            chips[2] *= -0.2;
            let nibble = three_of_six(&chips[..6]);
            assert_eq!(nibble.value, byte >> 4);
            assert!(nibble.repaired);
        }
    }

    #[test]
    fn manchester_reads_a_rising_pair_as_one() {
        let chips = soft(0b0101_1010_0110_1001, 16);
        assert_eq!(manchester(&chips), 0b1100_1001);
    }

    #[test]
    fn the_block_crc_matches_a_recorded_header() {
        let block = [0x4E, 0x44, 0xB4, 0x09, 0x33, 0x23, 0x16, 0x18, 0x13, 0x07];
        let check = crc(&block).to_be_bytes();
        let mut framed = block.to_vec();
        framed.extend(check);
        assert!(crc_holds(&framed, 10));
        framed[3] ^= 1;
        assert!(!crc_holds(&framed, 10));
    }
}

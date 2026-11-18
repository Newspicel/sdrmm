use sdrmm_dsp::crc16_msb;

const CRC_POLY: u16 = 0x3D65;

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
    !crc16_msb(CRC_POLY, 0, bytes)
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

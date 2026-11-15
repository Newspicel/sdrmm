#[must_use]
pub fn reverse_byte(b: u8) -> u8 {
    b.reverse_bits()
}

#[must_use]
pub fn pack_msb(bits: &[bool]) -> Vec<u8> {
    let mut out = Vec::new();
    pack_msb_into(bits, &mut out);
    out
}

/// [`pack_msb`] into a buffer the caller keeps, so a decoder packing every frame does not
/// allocate once the buffer has grown.
pub fn pack_msb_into(bits: &[bool], out: &mut Vec<u8>) {
    out.clear();
    out.resize(bits.len().div_ceil(8), 0);
    for (i, &bit) in bits.iter().enumerate() {
        out[i / 8] |= u8::from(bit) << (7 - i % 8);
    }
}

/// Bits read most significant first into one integer. Past 64 bits the oldest shift out.
#[must_use]
pub fn word_msb(bits: &[bool]) -> u64 {
    bits.iter().fold(0, |word, &bit| word << 1 | u64::from(bit))
}

#[must_use]
pub fn pack_lsb(bits: &[bool]) -> Vec<u8> {
    let mut out = vec![0u8; bits.len().div_ceil(8)];
    for (i, &bit) in bits.iter().enumerate() {
        out[i / 8] |= u8::from(bit) << (i % 8);
    }
    out
}

#[must_use]
pub fn bits_be(bytes: &[u8], offset: usize, len: usize) -> u64 {
    if len == 0 || len > 64 {
        return 0;
    }
    let (Some(end), Some(available)) = (offset.checked_add(len), bytes.len().checked_mul(8)) else {
        return 0;
    };
    if end > available {
        return 0;
    }
    let mut value = 0u64;
    for i in offset..end {
        value = (value << 1) | u64::from(bytes[i / 8] >> (7 - i % 8) & 1);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_reads_its_first_bit_as_the_most_significant() {
        assert_eq!(word_msb(&[]), 0);
        assert_eq!(word_msb(&[true, false, true, true]), 0b1011);
        let long: Vec<bool> = (0..70).map(|i| i >= 6).collect();
        assert_eq!(word_msb(&long), u64::MAX);
    }

    #[test]
    fn packing_into_a_kept_buffer_matches_a_fresh_one_and_pads_the_last_byte() {
        let bits: Vec<bool> = (0..13).map(|i| i % 3 == 0).collect();
        let mut out = vec![0xFF; 7];
        pack_msb_into(&bits, &mut out);
        assert_eq!(out, pack_msb(&bits));
        assert_eq!(out, [0b1001_0010, 0b0100_1000]);
    }

    #[test]
    fn reverse_byte_flips_bit_order() {
        assert_eq!(reverse_byte(0b1000_0001), 0b1000_0001);
        assert_eq!(reverse_byte(0b0101_0000), 0b0000_1010);
        assert_eq!(reverse_byte(0x01), 0x80);
    }

    #[test]
    fn pack_msb_and_lsb_are_bit_reverses_of_each_other() {
        let bits = [true, false, true, false, false, false, false, true];
        assert_eq!(pack_msb(&bits), [0xa1]);
        assert_eq!(pack_lsb(&bits), [0x85]);
        assert_eq!(pack_lsb(&bits)[0], reverse_byte(pack_msb(&bits)[0]));

        assert_eq!(pack_msb(&[true; 3]), [0xe0]);
        assert_eq!(pack_lsb(&[true; 3]), [0x07]);
        assert!(pack_msb(&[]).is_empty());
    }

    #[test]
    fn bits_be_reads_fields_across_byte_boundaries() {
        let bytes = [0xa1u8, 0x5c];
        assert_eq!(bits_be(&bytes, 0, 8), 0xa1);
        assert_eq!(bits_be(&bytes, 4, 8), 0x15);
        assert_eq!(bits_be(&bytes, 12, 4), 0b1100);
        assert_eq!(bits_be(&bytes, 0, 16), 0xa15c);
        assert_eq!(bits_be(&[0xff; 8], 0, 64), u64::MAX);
    }

    #[test]
    fn bits_be_returns_zero_past_the_end() {
        let bytes = [0xa1u8, 0x5c];
        assert_eq!(bits_be(&bytes, 12, 8), 0);
        assert_eq!(bits_be(&bytes, 16, 1), 0);
        assert_eq!(bits_be(&bytes, 0, 0), 0);
        assert_eq!(bits_be(&bytes, 0, 65), 0);
        assert_eq!(bits_be(&[], 0, 1), 0);
    }
}

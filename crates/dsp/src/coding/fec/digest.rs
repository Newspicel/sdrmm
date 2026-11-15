#[must_use]
pub fn lfsr_digest8(poly: u8, key: u8, data: &[u8]) -> u8 {
    digest_rolling_right(poly.into(), key.into(), data) as u8
}

#[must_use]
pub fn lfsr_digest16(poly: u16, key: u16, data: &[u8]) -> u16 {
    digest_rolling_right(poly.into(), key.into(), data) as u16
}

#[must_use]
pub fn lfsr_digest8_reflect(poly: u8, key: u8, data: &[u8]) -> u8 {
    let mut key = key;
    let mut sum = 0u8;
    for &byte in data.iter().rev() {
        for bit in 0..8 {
            if byte >> bit & 1 == 1 {
                sum ^= key;
            }
            key = if key & 0x80 == 0 {
                key << 1
            } else {
                (key << 1) ^ poly
            };
        }
    }
    sum
}

fn digest_rolling_right(poly: u32, key: u32, data: &[u8]) -> u32 {
    let mut key = key;
    let mut sum = 0;
    for &byte in data {
        for bit in (0..8).rev() {
            if byte >> bit & 1 == 1 {
                sum ^= key;
            }
            key = if key & 1 == 0 {
                key >> 1
            } else {
                (key >> 1) ^ poly
            };
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_lfsr_digest_of_one_set_bit_is_the_key_it_started_from() {
        assert_eq!(lfsr_digest8_reflect(0x31, 0xF4, &[]), 0x00);
        assert_eq!(lfsr_digest8_reflect(0x31, 0xF4, &[0x00]), 0x00);
        assert_eq!(lfsr_digest8_reflect(0x31, 0xF4, &[0x01]), 0xF4);
        assert_eq!(lfsr_digest8_reflect(0x31, 0xF4, &[0x80]), 0x23);
    }

    #[test]
    fn a_forward_lfsr_digest_of_one_set_bit_is_the_key_it_started_from() {
        assert_eq!(lfsr_digest8(0x98, 0x3E, &[]), 0x00);
        assert_eq!(lfsr_digest8(0x98, 0x3E, &[0x80]), 0x3E);
        assert_eq!(lfsr_digest8(0x98, 0xF1, &[0x7B, 0x80, 0xBB]), 0x76);
        assert_eq!(
            lfsr_digest8(0x98, 0x3E, &[0x45, 0x93, 0x24, 0x41, 0x30]),
            0x0B
        );
    }

    #[test]
    fn a_sixteen_bit_lfsr_digest_starts_from_its_key_and_rolls_right() {
        assert_eq!(lfsr_digest16(0x8810, 0x5412, &[]), 0x0000);
        assert_eq!(lfsr_digest16(0x8810, 0x5412, &[0x80]), 0x5412);
        assert_eq!(lfsr_digest16(0x8810, 0x5412, &[0x40]), 0x2A09);
        assert_eq!(lfsr_digest16(0x8810, 0x5412, &[0x20]), 0x9D14);
    }

    #[test]
    fn a_forward_lfsr_digest_is_linear_over_the_message_bits() {
        let left = [0x7Bu8, 0x80, 0xBB];
        let right = [0x2Eu8, 0x2F, 0x84];
        let mixed: Vec<u8> = left.iter().zip(right).map(|(&a, b)| a ^ b).collect();
        assert_eq!(
            lfsr_digest8(0x98, 0xF1, &mixed),
            lfsr_digest8(0x98, 0xF1, &left) ^ lfsr_digest8(0x98, 0xF1, &right)
        );
    }

    #[test]
    fn an_lfsr_digest_is_linear_over_the_message_bits() {
        let left = [0x8Fu8, 0x12, 0xC9, 0x2F];
        let right = [0x2Au8, 0xA1, 0xA9, 0x58];
        let mixed: Vec<u8> = left.iter().zip(right).map(|(&a, b)| a ^ b).collect();
        assert_eq!(
            lfsr_digest8_reflect(0x31, 0xF4, &mixed),
            lfsr_digest8_reflect(0x31, 0xF4, &left) ^ lfsr_digest8_reflect(0x31, 0xF4, &right)
        );
        assert_eq!(lfsr_digest8_reflect(0x31, 0xF4, &left), 0x9F);
        assert_eq!(lfsr_digest8_reflect(0x31, 0xF4, &right), 0x23);
    }
}

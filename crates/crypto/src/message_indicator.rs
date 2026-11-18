//! The message indicator a voice privacy superframe is ciphered from.

/// One step of the 64-bit message indicator LFSR of P25 and NXDN (x^64 + x^62 + x^46 + x^38 +
/// x^27 + x^15 + 1) clocked 64 times: the next superframe's MI when its own was lost, and the low
/// half of a 128-bit AES IV.
#[must_use]
pub fn next_message_indicator(mi: u64) -> u64 {
    (0..64).fold(mi, |lfsr, _| {
        let bit = (lfsr >> 63 ^ lfsr >> 61 ^ lfsr >> 45 ^ lfsr >> 37 ^ lfsr >> 26 ^ lfsr >> 14) & 1;
        lfsr << 1 | bit
    })
}

/// The 128-bit AES IV a 64-bit message indicator stands for: the MI and the LFSR's next 64 bits.
#[must_use]
pub fn aes_iv(mi: u64) -> [u8; 16] {
    (u128::from(mi) << 64 | u128::from(next_message_indicator(mi))).to_be_bytes()
}

#[cfg(test)]
mod tests {
    use super::{aes_iv, next_message_indicator};

    #[test]
    fn the_message_indicator_lfsr_steps_64_bits() {
        assert_eq!(
            next_message_indicator(0x0123_4567_89AB_CDEF),
            0x20B1_25E7_79D0_F34E
        );
        assert_eq!(
            aes_iv(0x0123_4567_89AB_CDEF),
            0x0123_4567_89AB_CDEF_20B1_25E7_79D0_F34E_u128.to_be_bytes()
        );
    }
}

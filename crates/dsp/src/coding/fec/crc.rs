const CCITT_POLY_REFLECTED: u32 = 0x8408;
const X25_INIT: u32 = 0xFFFF;
const CRC32_POLY: u32 = 0x04C1_1DB7;
const CRC32_POLY_REFLECTED: u32 = 0xEDB8_8320;
const CRC32_INIT: u32 = u32::MAX;

const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < table.len() {
        table[n] = shift_byte_reflected(n as u32, CRC32_POLY_REFLECTED);
        n += 1;
    }
    table
};

#[must_use]
pub fn crc16_x25(data: &[u8]) -> u16 {
    !(crc_reflected(CCITT_POLY_REFLECTED, X25_INIT, data) as u16)
}

#[must_use]
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    crc_reflected(CCITT_POLY_REFLECTED, 0, data) as u16
}

#[must_use]
pub fn crc16_msb_bits(poly: u16, init: u16, bits: &[bool]) -> u16 {
    crc_msb_bits(poly.into(), 16, init.into(), bits.iter().copied()) as u16
}

/// Bit-serial MSB-first CRC of any width from 1 to 32 bits, for codes whose protected field is
/// not a whole number of bytes. No final inversion: callers apply their own.
#[must_use]
pub fn crc_msb_bits(poly: u32, width: u32, init: u32, bits: impl IntoIterator<Item = bool>) -> u32 {
    let width = width.clamp(1, 32);
    let mask = u32::MAX >> (32 - width);
    let top = 1u32 << (width - 1);
    bits.into_iter().fold(init & mask, |crc, bit| {
        let feedback = (crc & top != 0) != bit;
        let shifted = (crc << 1) & mask;
        if feedback {
            shifted ^ (poly & mask)
        } else {
            shifted
        }
    })
}

#[must_use]
pub fn crc16_msb(poly: u16, init: u16, data: &[u8]) -> u16 {
    crc_msb_bits(poly.into(), 16, init.into(), msb_first(data)) as u16
}

#[must_use]
pub fn crc32_ieee(data: &[u8]) -> u32 {
    !data.iter().fold(CRC32_INIT, |crc, &byte| {
        CRC32_TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize] ^ (crc >> 8)
    })
}

#[must_use]
pub fn crc24_reflected(poly: u32, init: u32, data: &[u8]) -> u32 {
    crc_reflected(poly, init, data)
}

#[must_use]
pub fn crc32_mpeg(data: &[u8]) -> u32 {
    crc_msb_bits(CRC32_POLY, 32, CRC32_INIT, msb_first(data))
}

#[must_use]
pub fn crc8_msb(poly: u8, init: u8, data: &[u8]) -> u8 {
    crc_msb_bits(poly.into(), 8, init.into(), msb_first(data)) as u8
}

/// A four-bit CRC run as an eight-bit one with everything moved into the top nibble.
#[must_use]
pub fn crc4_msb(poly: u8, init: u8, data: &[u8]) -> u8 {
    crc8_msb(poly << 4, init << 4, data) >> 4
}

fn msb_first(data: &[u8]) -> impl Iterator<Item = bool> + '_ {
    data.iter()
        .flat_map(|&byte| (0..8).rev().map(move |shift| byte >> shift & 1 == 1))
}

fn crc_reflected(poly: u32, init: u32, data: &[u8]) -> u32 {
    data.iter().fold(init, |crc, &byte| {
        shift_byte_reflected(crc ^ u32::from(byte), poly)
    })
}

const fn shift_byte_reflected(mut crc: u32, poly: u32) -> u32 {
    let mut bit = 0;
    while bit < 8 {
        crc = if crc & 1 == 1 {
            (crc >> 1) ^ poly
        } else {
            crc >> 1
        };
        bit += 1;
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::{super::test_data::flip_msb_first, *};

    const CHECK: &[u8] = b"123456789";

    #[test]
    fn crc8_matches_the_published_catalogue_check_values() {
        assert_eq!(crc8_msb(0x07, 0x00, CHECK), 0xF4);
        assert_eq!(crc8_msb(0x31, 0xFF, CHECK), 0xF7);
        assert_eq!(crc8_msb(0x1D, 0xFD, CHECK), 0x7E);
    }

    #[test]
    fn a_bit_serial_crc_matches_the_published_catalogue_check_values() {
        let bits: Vec<bool> = msb_first(CHECK).collect();
        let crc = |poly, width, init| crc_msb_bits(poly, width, init, bits.iter().copied());
        assert_eq!(crc(0x07, 8, 0), 0xF4);
        assert_eq!(crc(0x1021, 16, 0xFFFF), 0x29B1);
        assert_eq!(crc(0x80F, 12, 0), 0xF5B);
        assert_eq!(crc(0x04C1_1DB7, 32, u32::MAX), 0x0376_E6E7);
        assert_eq!(crc16_msb_bits(0x1021, 0xFFFF, &bits), 0x29B1);
    }

    #[test]
    fn a_bit_serial_crc_agrees_with_the_byte_wise_one() {
        let data = [0xDE, 0xAD, 0xBE, 0xEF, 0x01];
        assert_eq!(
            crc_msb_bits(0x07, 8, 0x5A, msb_first(&data)),
            u32::from(crc8_msb(0x07, 0x5A, &data))
        );
        assert_eq!(crc16_msb(0x1021, 0xFFFF, CHECK), 0x29B1);
        assert_eq!(crc32_mpeg(CHECK), 0x0376_E6E7);
    }

    #[test]
    fn crc4_leaves_a_zero_message_at_its_starting_value() {
        assert_eq!(crc4_msb(0x13, 0x0, &[0x00; 4]), 0x0);
        assert_eq!(crc4_msb(0x13, 0x0, &[0x01]), 0x3);
        assert_eq!(crc4_msb(0x13, 0x0, &[0x0F, 0x30, 0x65, 0x06]), 0xA);
    }

    #[test]
    fn crc4_never_returns_more_than_the_four_bits_it_names() {
        for byte in 0..=u8::MAX {
            assert!(crc4_msb(0x13, 0x0, &[byte]) <= 0x0F);
        }
    }

    #[test]
    fn crc32_ieee_matches_catalogue_check_value() {
        assert_eq!(crc32_ieee(CHECK), 0xCBF4_3926);
        assert_eq!(crc32_ieee(&[]), 0);
    }

    #[test]
    fn crc24_ble_matches_catalogue_check_value() {
        assert_eq!(crc24_reflected(0xDA_6000, 0xAA_AAAA, CHECK), 0xC2_5A56);
    }

    #[test]
    fn crc16_x25_matches_catalogue_check_value() {
        assert_eq!(crc16_x25(CHECK), 0x906E);
    }

    #[test]
    fn crc16_ccitt_matches_catalogue_check_value() {
        assert_eq!(crc16_ccitt(CHECK), 0x2189);
    }

    #[test]
    fn crc16_ccitt_check_bytes_leave_a_zero_remainder() {
        let message = b"2.D-AIBC\x01H1B\x02HELLO\x03";
        let mut with_check = message.to_vec();
        with_check.extend_from_slice(&crc16_ccitt(message).to_le_bytes());
        assert_eq!(crc16_ccitt(&with_check), 0);

        for bit in 0..with_check.len() * 8 {
            let mut corrupt = with_check.clone();
            flip_msb_first(&mut corrupt, bit);
            assert_ne!(crc16_ccitt(&corrupt), 0, "bit {bit} slipped past the CRC");
        }
    }
}

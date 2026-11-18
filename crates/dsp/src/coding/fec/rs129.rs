use super::rs256::GF256_PRIMITIVE;

const GENERATOR: [u8; 3] = [64, 56, 14];

/// The three parity bytes of the RS(12,9) code over GF(256).
#[must_use]
pub fn rs129_parity(msg: &[u8]) -> [u8; 3] {
    let mut parity = [0u8; 3];
    for &byte in msg {
        let feedback = byte ^ parity[2];
        parity[2] = parity[1] ^ mul(GENERATOR[2], feedback);
        parity[1] = parity[0] ^ mul(GENERATOR[1], feedback);
        parity[0] = mul(GENERATOR[0], feedback);
    }
    [parity[2], parity[1], parity[0]]
}

/// Bit-serial, so a three-tap code needs no log tables.
fn mul(mut a: u8, mut b: u8) -> u8 {
    let reduction = GF256_PRIMITIVE as u8;
    let mut product = 0u8;
    while b != 0 {
        if b & 1 != 0 {
            product ^= a;
        }
        let high = a & 0x80 != 0;
        a <<= 1;
        if high {
            a ^= reduction;
        }
        b >>= 1;
    }
    product
}

#[cfg(test)]
mod tests {
    use super::{super::rs256::ReedSolomon, *};

    #[test]
    fn the_parity_matches_the_general_code_with_roots_from_x() {
        let code = ReedSolomon::new(GF256_PRIMITIVE, 1, 3);
        let msg = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x0F];
        let mut codeword = Vec::new();
        code.encode(&msg, &mut codeword);
        assert_eq!(rs129_parity(&msg), codeword[9..]);
    }

    #[test]
    fn multiplication_inverts_through_the_field() {
        for value in 1..=u8::MAX {
            let inverse = (1..=u8::MAX).find(|&other| mul(value, other) == 1);
            assert!(inverse.is_some(), "{value} has no inverse");
        }
    }
}

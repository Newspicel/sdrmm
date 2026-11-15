use super::gf::SmallField;

const FIELD: SmallField = SmallField::GF256;
const GENERATOR: [u8; 3] = [64, 56, 14];

/// The three parity bytes of the RS(12,9) code over GF(256).
#[must_use]
pub fn rs129_parity(msg: &[u8]) -> [u8; 3] {
    let mut parity = [0u8; 3];
    for &byte in msg {
        let feedback = byte ^ parity[2];
        parity[2] = parity[1] ^ FIELD.mul(GENERATOR[2], feedback);
        parity[1] = parity[0] ^ FIELD.mul(GENERATOR[1], feedback);
        parity[0] = FIELD.mul(GENERATOR[0], feedback);
    }
    [parity[2], parity[1], parity[0]]
}

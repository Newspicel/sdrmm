use std::sync::LazyLock;

use sdrmm_dsp::{GF256_PRIMITIVE, ReedSolomon, crc24_reflected};

pub const RS6_N: usize = 52;
const RS6_NPAR: usize = 10;
const RS6_FCR: u8 = 54;
const GF64_PRIMITIVE: u16 = 0x43;

const RS8_DATA: usize = 31;
const RS8_SENT: usize = 39;
const RS8_OFFSET: usize = 208;
const RS8_ERASURES: [usize; 8] = [247, 248, 249, 250, 251, 252, 253, 254];

static RS6: LazyLock<ReedSolomon> =
    LazyLock::new(|| ReedSolomon::with_field(6, GF64_PRIMITIVE, RS6_FCR, RS6_NPAR));
static RS8: LazyLock<ReedSolomon> = LazyLock::new(|| ReedSolomon::new(GF256_PRIMITIVE, 0, 16));

pub fn rs6_correct(cw: &mut [u8; RS6_N]) -> Option<u32> {
    RS6.decode(cw)
}

pub fn rs8_correct(sent: &[u8; RS8_SENT]) -> Option<[u8; RS8_DATA]> {
    let mut cw = [0u8; 255];
    cw[RS8_OFFSET..RS8_OFFSET + RS8_SENT].copy_from_slice(sent);
    RS8.decode_with_erasures(&mut cw, &RS8_ERASURES)?;
    let mut out = [0u8; RS8_DATA];
    out.copy_from_slice(&cw[RS8_OFFSET..RS8_OFFSET + RS8_DATA]);
    Some(out)
}

pub fn iip_crc24(data: &[u8]) -> u32 {
    const REVERSED_POLY: u32 = 0xAD85DD;
    crc24_reflected(REVERSED_POLY, 0xFF_FFFF, data) ^ 0x0C91B6
}

pub fn checksum_16(msg: &[u8; RS8_DATA]) -> u16 {
    let mut sum: u32 = msg[..28]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|w| u32::from(u16::from_le_bytes([w[0], w[1]])))
        .sum();
    sum += u32::from(msg[28]);
    sum += u32::from(u16::from_le_bytes([msg[29], msg[30]]));
    let folded = (sum & 0xFFFF) + (sum >> 16);
    (folded as u16) ^ 0xFFFF
}

pub fn bytes_of_bits(bits: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let straight: Vec<u8> = bits
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| c.iter().fold(0u8, |v, &b| (v << 1) | b))
        .collect();
    let reversed = straight.iter().map(|b| b.reverse_bits()).collect();
    (straight, reversed)
}

pub fn symbols6(bits: &[u8]) -> [u8; RS6_N] {
    let mut cw = [0u8; RS6_N];
    for (symbol, chunk) in cw.iter_mut().zip(bits.chunks(6)) {
        *symbol = chunk.iter().fold(0u8, |v, &b| (v << 1) | b);
    }
    cw
}

pub fn first_39(bytes: &[u8]) -> Option<[u8; RS8_SENT]> {
    bytes.get(..RS8_SENT)?.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc24_check_value() {
        assert_eq!(iip_crc24(b"123456789"), 0xbde882);
        assert_eq!(iip_crc24(b""), 0xf36e49);
    }
}

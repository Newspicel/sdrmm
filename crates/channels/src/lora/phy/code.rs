use sdrmm_wire::LoraCodingRate;

pub(crate) const HEADER_NIBBLES: usize = 5;
pub(crate) const HEADER_SYMBOLS: usize = 8;
pub(crate) const HEADER_PARITY_BITS: u8 = 4;
pub(crate) const MAX_PAYLOAD: usize = 255;
pub(crate) const CRC_NIBBLES: usize = 4;
const LOW_DATA_RATE_SYMBOL_S: f64 = 0.016;

pub(crate) static WHITENING: [u8; MAX_PAYLOAD] = whitening_table();

const fn whitening_table() -> [u8; MAX_PAYLOAD] {
    let mut table = [0u8; MAX_PAYLOAD];
    let mut state: u8 = 0xff;
    let mut i = 0;
    while i < MAX_PAYLOAD {
        table[i] = state;
        let feedback = ((state >> 7) ^ (state >> 5) ^ (state >> 4) ^ (state >> 3)) & 1;
        state = (state << 1) | feedback;
        i += 1;
    }
    table
}

#[must_use]
pub(crate) fn low_data_rate(spreading_factor: u8, bandwidth_hz: f64) -> bool {
    f64::from(1u32 << spreading_factor) / bandwidth_hz > LOW_DATA_RATE_SYMBOL_S
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Header {
    pub length: u8,
    pub coding_rate: LoraCodingRate,
    pub crc: bool,
}

fn bit(value: u8, index: u8) -> u8 {
    (value >> index) & 1
}

#[must_use]
pub(crate) fn header_checksum(n0: u8, n1: u8, n2: u8) -> u8 {
    let c4 = bit(n0, 3) ^ bit(n0, 2) ^ bit(n0, 1) ^ bit(n0, 0);
    let c3 = bit(n0, 3) ^ bit(n1, 3) ^ bit(n1, 2) ^ bit(n1, 1) ^ bit(n2, 0);
    let c2 = bit(n0, 2) ^ bit(n1, 3) ^ bit(n1, 0) ^ bit(n2, 3) ^ bit(n2, 1);
    let c1 = bit(n0, 1) ^ bit(n1, 2) ^ bit(n1, 0) ^ bit(n2, 2) ^ bit(n2, 1) ^ bit(n2, 0);
    let c0 = bit(n0, 0) ^ bit(n1, 1) ^ bit(n2, 3) ^ bit(n2, 2) ^ bit(n2, 1) ^ bit(n2, 0);
    (c4 << 4) | (c3 << 3) | (c2 << 2) | (c1 << 1) | c0
}

impl Header {
    #[must_use]
    pub(crate) fn parse(nibbles: &[u8]) -> Option<Self> {
        let [n0, n1, n2, n3, n4] = *nibbles.get(..HEADER_NIBBLES)? else {
            return None;
        };
        let checksum = ((n3 & 0x0f) << 4) | (n4 & 0x0f);
        let length = (n0 << 4) | n1;
        let coding_rate = LoraCodingRate::from_parity_bits(n2 >> 1)?;
        (checksum == header_checksum(n0, n1, n2) && length > 0).then_some(Self {
            length,
            coding_rate,
            crc: n2 & 1 == 1,
        })
    }

    #[must_use]
    pub(crate) fn nibbles(self) -> [u8; HEADER_NIBBLES] {
        let n0 = self.length >> 4;
        let n1 = self.length & 0x0f;
        let n2 = (self.coding_rate.parity_bits() << 1) | u8::from(self.crc);
        let checksum = header_checksum(n0, n1, n2);
        [n0, n1, n2, checksum >> 4, checksum & 0x0f]
    }

    #[must_use]
    pub(crate) fn payload_nibbles(self) -> usize {
        2 * usize::from(self.length) + if self.crc { CRC_NIBBLES } else { 0 }
    }
}

fn crc16_ccitt(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[must_use]
pub(crate) fn payload_crc(payload: &[u8]) -> u16 {
    let (body, tail) = payload.split_at(payload.len().saturating_sub(2));
    let last = tail.last().copied().map_or(0, u16::from);
    let before = if tail.len() == 2 {
        u16::from(tail[0]) << 8
    } else {
        0
    };
    crc16_ccitt(body) ^ last ^ before
}

#[must_use]
pub(crate) fn gray(value: u16) -> u16 {
    value ^ (value >> 1)
}

#[must_use]
pub(crate) fn gray_inverse(value: u16) -> u16 {
    let mut out = value;
    let mut shift = value >> 1;
    while shift != 0 {
        out ^= shift;
        shift >>= 1;
    }
    out
}

#[must_use]
pub(crate) fn codeword_len(parity_bits: u8) -> usize {
    4 + usize::from(parity_bits)
}

#[must_use]
pub(crate) fn hamming_encode(nibble: u8, parity_bits: u8) -> u8 {
    let b = |i| bit(nibble, i);
    let data = (b(0) << 3) | (b(1) << 2) | (b(2) << 1) | b(3);
    if parity_bits == 1 {
        return (data << 1) | (b(0) ^ b(1) ^ b(2) ^ b(3));
    }
    let p0 = b(0) ^ b(1) ^ b(2);
    let p1 = b(1) ^ b(2) ^ b(3);
    let p2 = b(0) ^ b(1) ^ b(3);
    let p3 = b(0) ^ b(2) ^ b(3);
    let full = (data << 4) | (p0 << 3) | (p1 << 2) | (p2 << 1) | p3;
    full >> (4 - parity_bits)
}

#[must_use]
pub(crate) fn codeword_bit(codeword: u8, len: usize, index: usize) -> bool {
    (codeword >> (len - 1 - index)) & 1 == 1
}

#[must_use]
pub(crate) fn diagonal(index: usize, symbol: usize, rows: usize) -> usize {
    (symbol + 2 * rows - index - 1) % rows
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shape {
    pub spreading_factor: u8,
    pub coding_rate: LoraCodingRate,
    pub low_data_rate: bool,
}

impl Shape {
    #[must_use]
    pub(crate) fn first_rows(self) -> usize {
        usize::from(self.spreading_factor) - 2
    }

    #[must_use]
    pub(crate) fn rows(self) -> usize {
        usize::from(self.spreading_factor) - if self.low_data_rate { 2 } else { 0 }
    }

    #[must_use]
    pub(crate) fn columns(self) -> usize {
        codeword_len(self.coding_rate.parity_bits())
    }

    #[must_use]
    pub(crate) fn symbols_for(self, nibbles: usize) -> usize {
        let rest = nibbles.saturating_sub(self.first_rows());
        HEADER_SYMBOLS + rest.div_ceil(self.rows()) * self.columns()
    }
}

#[must_use]
pub(crate) fn dewhiten(nibbles: &[u8], length: usize) -> Vec<u8> {
    (0..length)
        .map(|i| {
            let low = nibbles.get(2 * i).copied().unwrap_or(0);
            let high = nibbles.get(2 * i + 1).copied().unwrap_or(0);
            ((high << 4) | low) ^ WHITENING[i % MAX_PAYLOAD]
        })
        .collect()
}

#[must_use]
pub(crate) fn received_crc(nibbles: &[u8], length: usize) -> Option<u16> {
    let crc = nibbles.get(2 * length..2 * length + CRC_NIBBLES)?;
    Some(
        crc.iter()
            .enumerate()
            .fold(0u16, |acc, (i, &n)| acc | (u16::from(n & 0x0f) << (4 * i))),
    )
}

#[cfg(any(test, feature = "synth"))]
pub(crate) mod encode {
    use super::{
        CRC_NIBBLES, HEADER_PARITY_BITS, Header, Shape, WHITENING, codeword_bit, codeword_len,
        diagonal, gray_inverse, hamming_encode, payload_crc,
    };

    #[must_use]
    pub(crate) fn nibbles(payload: &[u8], header: Option<Header>, crc: bool) -> Vec<u8> {
        let mut out: Vec<u8> = header.map(|h| h.nibbles().to_vec()).unwrap_or_default();
        for (i, &byte) in payload.iter().enumerate() {
            let white = byte ^ WHITENING[i % WHITENING.len()];
            out.push(white & 0x0f);
            out.push(white >> 4);
        }
        if crc {
            let value = payload_crc(payload);
            out.extend((0..CRC_NIBBLES).map(|i| ((value >> (4 * i)) & 0x0f) as u8));
        }
        out
    }

    fn interleave(
        codewords: &[u8],
        columns: usize,
        rows: usize,
        reduced: bool,
        sf: u8,
    ) -> Vec<u16> {
        (0..columns)
            .map(|symbol| {
                let value = (0..rows).fold(0u16, |acc, j| {
                    let codeword = codewords[diagonal(j, symbol, rows)];
                    let set = codeword_bit(codeword, columns, symbol);
                    acc | (u16::from(set) << (rows - 1 - j))
                });
                let shifted = if reduced {
                    gray_inverse(value) << 2
                } else {
                    gray_inverse(value)
                };
                (shifted + 1) & ((1u16 << sf) - 1)
            })
            .collect()
    }

    #[must_use]
    pub(crate) fn symbols(nibbles: &[u8], shape: Shape) -> Vec<u16> {
        let sf = shape.spreading_factor;
        let first = shape.first_rows();
        let mut out = Vec::new();
        let head: Vec<u8> = (0..first)
            .map(|i| hamming_encode(nibbles.get(i).copied().unwrap_or(0), HEADER_PARITY_BITS))
            .collect();
        out.extend(interleave(
            &head,
            codeword_len(HEADER_PARITY_BITS),
            first,
            true,
            sf,
        ));
        let rest = nibbles.get(first..).unwrap_or_default();
        let parity = shape.coding_rate.parity_bits();
        for block in rest.chunks(shape.rows()) {
            let codewords: Vec<u8> = (0..shape.rows())
                .map(|i| hamming_encode(block.get(i).copied().unwrap_or(0), parity))
                .collect();
            out.extend(interleave(
                &codewords,
                shape.columns(),
                shape.rows(),
                shape.low_data_rate,
                sf,
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitening_follows_the_sx127x_sequence() {
        assert_eq!(
            WHITENING[..16],
            [
                0xff, 0xfe, 0xfc, 0xf8, 0xf0, 0xe1, 0xc2, 0x85, 0x0b, 0x17, 0x2f, 0x5e, 0xbc, 0x78,
                0xf1, 0xe3
            ]
        );
        assert_eq!(WHITENING[MAX_PAYLOAD - 5..], [0x27, 0x4f, 0x9f, 0x3f, 0x7f]);
    }

    #[test]
    fn the_crc_of_deadbeef_matches_an_rn2483_capture() {
        let crc = payload_crc(&[0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(crc, 0xec80);
        let printed_whitened = [
            (crc & 0xff) as u8 ^ WHITENING[4],
            (crc >> 8) as u8 ^ WHITENING[5],
        ];
        assert_eq!(printed_whitened, [0x70, 0x0d]);
    }

    #[test]
    fn a_header_round_trips_and_rejects_a_flipped_bit() {
        let header = Header {
            length: 4,
            coding_rate: LoraCodingRate::Cr48,
            crc: true,
        };
        let nibbles = header.nibbles();
        assert_eq!(nibbles[..3], [0, 4, 9]);
        assert_eq!(Header::parse(&nibbles), Some(header));
        for index in 0..HEADER_NIBBLES {
            for flip in 0..4 {
                let mut damaged = nibbles;
                damaged[index] ^= 1 << flip;
                assert_ne!(Header::parse(&damaged), Some(header), "{index} {flip}");
            }
        }
    }

    #[test]
    fn gray_and_its_inverse_undo_each_other() {
        for value in 0..4096u16 {
            assert_eq!(gray(gray_inverse(value)), value);
            assert_eq!(gray_inverse(gray(value)), value);
        }
    }

    #[test]
    fn hamming_codewords_have_their_documented_shape() {
        assert_eq!(hamming_encode(0b0001, 4), 0b1000_1011);
        assert_eq!(hamming_encode(0b1111, 4), 0xff);
        assert_eq!(hamming_encode(0b0001, 1), 0b10001);
        for nibble in 0..16 {
            for parity in 1..=4 {
                let codeword = hamming_encode(nibble, parity);
                assert!(u32::from(codeword) < 1 << codeword_len(parity));
                let data = (0..4).fold(0, |acc, i| {
                    acc | (u8::from(codeword_bit(codeword, codeword_len(parity), i)) << i)
                });
                assert_eq!(data, nibble);
            }
        }
    }

    #[test]
    fn the_symbol_count_matches_the_semtech_airtime_formula() {
        let shape = Shape {
            spreading_factor: 7,
            coding_rate: LoraCodingRate::Cr48,
            low_data_rate: false,
        };
        let header = Header {
            length: 4,
            coding_rate: LoraCodingRate::Cr48,
            crc: true,
        };
        assert_eq!(
            shape.symbols_for(HEADER_NIBBLES + header.payload_nibbles()),
            24
        );
        let slow = Shape {
            spreading_factor: 12,
            coding_rate: LoraCodingRate::Cr45,
            low_data_rate: true,
        };
        let payload = 2 * 20 + CRC_NIBBLES;
        let semtech = 8 + (156usize).div_ceil(40) * 5;
        assert_eq!(slow.symbols_for(HEADER_NIBBLES + payload), semtech);
    }

    #[test]
    fn dewhitening_inverts_the_encoder() {
        let payload = b"hello lora";
        let nibbles = encode::nibbles(payload, None, true);
        assert_eq!(dewhiten(&nibbles, payload.len()), payload);
        assert_eq!(
            received_crc(&nibbles, payload.len()),
            Some(payload_crc(payload))
        );
    }
}

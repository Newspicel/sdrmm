use std::sync::LazyLock;

pub(crate) const B_BITS: usize = 320;
pub(crate) const X_BITS: usize = 4;
pub(crate) const FIELD_BITS: usize = B_BITS + X_BITS;
pub(crate) const B_BYTES: usize = B_BITS / 8;
pub(crate) const ADPCM_SAMPLES: usize = B_BITS / 4;
const FIELD_BYTES: usize = FIELD_BITS.div_ceil(8);
const SCRAMBLERS: usize = 8;
const X_TEST_RUNS: usize = 5;
const X_TEST_RUN: usize = 16;
const X_TEST_STRIDE: usize = 48;

static SCRAMBLE: LazyLock<[[u8; B_BYTES]; SCRAMBLERS]> =
    LazyLock::new(|| std::array::from_fn(|frame| scramble_sequence(frame as u8)));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BField {
    bytes: [u8; FIELD_BYTES],
}

impl BField {
    pub fn from_bits(bits: impl IntoIterator<Item = bool>) -> Self {
        let mut bytes = [0u8; FIELD_BYTES];
        for (index, bit) in bits.into_iter().take(FIELD_BITS).enumerate() {
            if bit {
                bytes[index / 8] |= 0x80 >> (index % 8);
            }
        }
        Self { bytes }
    }

    pub fn bit(&self, index: usize) -> bool {
        self.bytes[index / 8] & (0x80 >> (index % 8)) != 0
    }

    pub fn x_crc_ok(&self) -> bool {
        x_crc(self) == self.x_field()
    }

    fn x_field(&self) -> u8 {
        (0..X_BITS).fold(0, |acc, index| {
            (acc << 1) | u8::from(self.bit(B_BITS + index))
        })
    }

    pub fn descrambled(&self, frame: u8) -> [u8; B_BYTES] {
        let sequence = &SCRAMBLE[usize::from(frame) % SCRAMBLERS];
        std::array::from_fn(|index| self.bytes[index] ^ sequence[index])
    }

    #[cfg(any(test, feature = "synth"))]
    pub fn scrambled(data: &[u8; B_BYTES], frame: u8) -> Self {
        let sequence = &SCRAMBLE[usize::from(frame) % SCRAMBLERS];
        let mut bytes = [0u8; FIELD_BYTES];
        for (index, byte) in bytes.iter_mut().take(B_BYTES).enumerate() {
            *byte = data[index] ^ sequence[index];
        }
        let mut field = Self { bytes };
        field.bytes[B_BYTES] = x_crc(&field) << 4;
        field
    }
}

fn x_crc(field: &BField) -> u8 {
    let mut remainder = 0u8;
    for run in 0..X_TEST_RUNS {
        let start = run * X_TEST_RUN + X_TEST_STRIDE * (1 + run);
        for nibble in 0..X_TEST_RUN / 4 {
            let at = start + nibble * 4;
            remainder ^= (0..4).fold(0, |acc, bit| (acc << 1) | u8::from(field.bit(at + bit)));
        }
    }
    remainder
}

pub(crate) fn adpcm_codes(data: &[u8; B_BYTES]) -> impl Iterator<Item = u8> + '_ {
    data.iter().flat_map(|&byte| [byte >> 4, byte & 0x0F])
}

fn scramble_sequence(frame: u8) -> [u8; B_BYTES] {
    let mut register = [frame & 1 == 1, frame & 2 == 2, frame & 4 == 4, true, true];
    let mut invert = true;
    let mut out = [0u8; B_BYTES];
    for index in 0..B_BITS {
        if register[4] ^ invert {
            out[index / 8] |= 0x80 >> (index % 8);
        }
        let leaving_all_ones = register.iter().all(|&q| q);
        let feedback = register[1] ^ register[4];
        register.rotate_right(1);
        register[0] = feedback;
        if leaving_all_ones {
            invert = !invert;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{B_BITS, B_BYTES, BField, FIELD_BITS, adpcm_codes, scramble_sequence};

    const ANNEX_E: [(usize, &str); 21] = [
        (0, "00000000"),
        (1, "00000001"),
        (2, "11110001"),
        (3, "11001101"),
        (4, "10101011"),
        (5, "00110010"),
        (6, "11010100"),
        (7, "10111101"),
        (8, "11100111"),
        (9, "11111000"),
        (10, "00111111"),
        (11, "01010110"),
        (12, "11111100"),
        (13, "11001111"),
        (14, "01101010"),
        (15, "10011010"),
        (78, "01010110"),
        (79, "00001100"),
        (317, "10111101"),
        (318, "11100111"),
        (319, "11111000"),
    ];

    fn bit(sequence: &[u8; B_BYTES], index: usize) -> char {
        if sequence[index / 8] & (0x80 >> (index % 8)) != 0 {
            '1'
        } else {
            '0'
        }
    }

    #[test]
    fn the_scrambler_reproduces_annex_e_for_every_frame() {
        for frame in 0..8u8 {
            let sequence = scramble_sequence(frame);
            for (index, row) in ANNEX_E {
                let expected = row.as_bytes()[usize::from(frame)] as char;
                assert_eq!(bit(&sequence, index), expected, "frame {frame} bit {index}");
            }
        }
    }

    #[test]
    fn frames_eight_apart_share_a_sequence() {
        let data = [0x5Au8; B_BYTES];
        let field = BField::scrambled(&data, 3);
        assert_eq!(field.descrambled(11), data);
        assert_ne!(field.descrambled(4), data);
    }

    #[test]
    fn the_x_crc_accepts_its_own_field_and_rejects_a_flipped_test_bit() {
        let data: [u8; B_BYTES] = std::array::from_fn(|index| (index * 37 + 11) as u8);
        let field = BField::scrambled(&data, 5);
        assert!(field.x_crc_ok());
        let bits: Vec<bool> = (0..FIELD_BITS).map(|index| field.bit(index)).collect();
        for tested in [48, 63, 112, 191, 240, 319, 321] {
            let mut flipped = bits.clone();
            flipped[tested] = !flipped[tested];
            assert!(!BField::from_bits(flipped).x_crc_ok(), "bit {tested}");
        }
        let mut untested = bits.clone();
        untested[0] = !untested[0];
        assert!(BField::from_bits(untested).x_crc_ok());
    }

    #[test]
    fn adpcm_codes_come_out_most_significant_nibble_first() {
        let mut data = [0u8; B_BYTES];
        data[0] = 0x1F;
        data[B_BYTES - 1] = 0xA2;
        let codes: Vec<u8> = adpcm_codes(&data).collect();
        assert_eq!(codes.len(), B_BITS / 4);
        assert_eq!(&codes[..2], &[0x1, 0xF]);
        assert_eq!(&codes[codes.len() - 2..], &[0xA, 0x2]);
    }
}

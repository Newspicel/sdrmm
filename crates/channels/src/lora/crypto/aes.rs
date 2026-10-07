const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

const INVERSE_SBOX: [u8; 256] = invert_table(&SBOX);

const fn invert_table(table: &[u8; 256]) -> [u8; 256] {
    let mut inverse = [0u8; 256];
    let mut index = 0;
    while index < 256 {
        inverse[table[index] as usize] = index as u8;
        index += 1;
    }
    inverse
}

const BLOCK: usize = 16;
const MAX_ROUNDS: usize = 14;

pub(crate) struct Aes {
    round_keys: [[u8; BLOCK]; MAX_ROUNDS + 1],
    rounds: usize,
}

impl Aes {
    pub(crate) fn new(key: &[u8]) -> Option<Self> {
        let rounds = match key.len() {
            16 => 10,
            32 => 14,
            _ => return None,
        };
        Some(Self {
            round_keys: expand_key(key, rounds),
            rounds,
        })
    }

    pub(crate) fn encrypt(&self, block: &mut [u8; BLOCK]) {
        add_round_key(block, &self.round_keys[0]);
        for round in 1..self.rounds {
            sub_bytes(block, &SBOX);
            shift_rows(block);
            mix_columns(block);
            add_round_key(block, &self.round_keys[round]);
        }
        sub_bytes(block, &SBOX);
        shift_rows(block);
        add_round_key(block, &self.round_keys[self.rounds]);
    }

    pub(crate) fn decrypt(&self, block: &mut [u8; BLOCK]) {
        add_round_key(block, &self.round_keys[self.rounds]);
        for round in (1..self.rounds).rev() {
            inverse_shift_rows(block);
            sub_bytes(block, &INVERSE_SBOX);
            add_round_key(block, &self.round_keys[round]);
            inverse_mix_columns(block);
        }
        inverse_shift_rows(block);
        sub_bytes(block, &INVERSE_SBOX);
        add_round_key(block, &self.round_keys[0]);
    }
}

fn expand_key(key: &[u8], rounds: usize) -> [[u8; BLOCK]; MAX_ROUNDS + 1] {
    let key_words = key.len() / 4;
    let total_words = 4 * (rounds + 1);
    let mut words = [[0u8; 4]; 4 * (MAX_ROUNDS + 1)];
    for (word, chunk) in words.iter_mut().zip(key.as_chunks::<4>().0) {
        *word = *chunk;
    }
    let mut round_constant = 1u8;
    for index in key_words..total_words {
        let mut word = words[index - 1];
        if index % key_words == 0 {
            word.rotate_left(1);
            word = word.map(|byte| SBOX[usize::from(byte)]);
            word[0] ^= round_constant;
            round_constant = double(round_constant);
        } else if key_words > 6 && index % key_words == 4 {
            word = word.map(|byte| SBOX[usize::from(byte)]);
        }
        let previous = words[index - key_words];
        words[index] = std::array::from_fn(|byte| previous[byte] ^ word[byte]);
    }
    std::array::from_fn(|round| std::array::from_fn(|byte| words[4 * round + byte / 4][byte % 4]))
}

fn add_round_key(block: &mut [u8; BLOCK], round_key: &[u8; BLOCK]) {
    for (byte, key_byte) in block.iter_mut().zip(round_key) {
        *byte ^= key_byte;
    }
}

fn sub_bytes(block: &mut [u8; BLOCK], table: &[u8; 256]) {
    for byte in block.iter_mut() {
        *byte = table[usize::from(*byte)];
    }
}

fn shift_rows(block: &mut [u8; BLOCK]) {
    let state = *block;
    for column in 0..4 {
        for row in 0..4 {
            block[row + 4 * column] = state[row + 4 * ((column + row) % 4)];
        }
    }
}

fn inverse_shift_rows(block: &mut [u8; BLOCK]) {
    let state = *block;
    for column in 0..4 {
        for row in 0..4 {
            block[row + 4 * ((column + row) % 4)] = state[row + 4 * column];
        }
    }
}

fn mix_columns(block: &mut [u8; BLOCK]) {
    transform_columns(block, [2, 3, 1, 1]);
}

fn inverse_mix_columns(block: &mut [u8; BLOCK]) {
    transform_columns(block, [14, 11, 13, 9]);
}

fn transform_columns(block: &mut [u8; BLOCK], coefficients: [u8; 4]) {
    for column in block.as_chunks_mut::<4>().0 {
        let input = *column;
        for (row, output) in column.iter_mut().enumerate() {
            *output = (0..4).fold(0, |sum, index| {
                sum ^ multiply(coefficients[(index + 4 - row) % 4], input[index])
            });
        }
    }
}

fn double(value: u8) -> u8 {
    (value << 1) ^ if value & 0x80 != 0 { 0x1b } else { 0 }
}

fn multiply(mut left: u8, mut right: u8) -> u8 {
    let mut product = 0;
    while right != 0 {
        if right & 1 != 0 {
            product ^= left;
        }
        left = double(left);
        right >>= 1;
    }
    product
}

#[cfg(test)]
mod tests {
    use super::Aes;
    use crate::lora::crypto::hex;

    fn block(text: &str) -> [u8; 16] {
        hex(text).unwrap().try_into().unwrap()
    }

    fn round_trip(key: &str, expected: &str) {
        let aes = Aes::new(&hex(key).unwrap()).unwrap();
        let plaintext = block("00112233445566778899aabbccddeeff");
        let mut data = plaintext;
        aes.encrypt(&mut data);
        assert_eq!(data, block(expected));
        aes.decrypt(&mut data);
        assert_eq!(data, plaintext);
    }

    #[test]
    fn aes128_fips197_appendix_c1() {
        round_trip(
            "000102030405060708090a0b0c0d0e0f",
            "69c4e0d86a7b0430d8cdb78070b4c55a",
        );
    }

    #[test]
    fn aes256_fips197_appendix_c3() {
        round_trip(
            "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
            "8ea2b7ca516745bfeafc49904b496089",
        );
    }

    #[test]
    fn rejects_unsupported_key_lengths() {
        assert!(Aes::new(&[0; 24]).is_none());
        assert!(Aes::new(&[0; 15]).is_none());
        assert!(Aes::new(&[]).is_none());
    }
}

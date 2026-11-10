/// DES (FIPS 46-3), encryption only: the land mobile radio ciphers run it in output feedback.
#[derive(Clone)]
pub(crate) struct Des {
    subkeys: [u64; ROUNDS],
}

const ROUNDS: usize = 16;

const INITIAL: [u8; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4, 62, 54, 46, 38, 30, 22, 14, 6,
    64, 56, 48, 40, 32, 24, 16, 8, 57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43, 35, 27, 19, 11, 3, 61,
    53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31, 23, 15, 7,
];

const FINAL: [u8; 64] = [
    40, 8, 48, 16, 56, 24, 64, 32, 39, 7, 47, 15, 55, 23, 63, 31, 38, 6, 46, 14, 54, 22, 62, 30,
    37, 5, 45, 13, 53, 21, 61, 29, 36, 4, 44, 12, 52, 20, 60, 28, 35, 3, 43, 11, 51, 19, 59, 27,
    34, 2, 42, 10, 50, 18, 58, 26, 33, 1, 41, 9, 49, 17, 57, 25,
];

const EXPANSION: [u8; 48] = [
    32, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8, 9, 8, 9, 10, 11, 12, 13, 12, 13, 14, 15, 16, 17, 16, 17, 18,
    19, 20, 21, 20, 21, 22, 23, 24, 25, 24, 25, 26, 27, 28, 29, 28, 29, 30, 31, 32, 1,
];

const PERMUTATION: [u8; 32] = [
    16, 7, 20, 21, 29, 12, 28, 17, 1, 15, 23, 26, 5, 18, 31, 10, 2, 8, 24, 14, 32, 27, 3, 9, 19,
    13, 30, 6, 22, 11, 4, 25,
];

const CHOICE_ONE: [u8; 56] = [
    57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59, 51, 43, 35, 27, 19, 11, 3, 60,
    52, 44, 36, 63, 55, 47, 39, 31, 23, 15, 7, 62, 54, 46, 38, 30, 22, 14, 6, 61, 53, 45, 37, 29,
    21, 13, 5, 28, 20, 12, 4,
];

const CHOICE_TWO: [u8; 48] = [
    14, 17, 11, 24, 1, 5, 3, 28, 15, 6, 21, 10, 23, 19, 12, 4, 26, 8, 16, 7, 27, 20, 13, 2, 41, 52,
    31, 37, 47, 55, 30, 40, 51, 45, 33, 48, 44, 49, 39, 56, 34, 53, 46, 42, 50, 36, 29, 32,
];

const SHIFTS: [u32; ROUNDS] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];

const SBOXES: [[u8; 64]; 8] = [
    [
        14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7, 0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12,
        11, 9, 5, 3, 8, 4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0, 15, 12, 8, 2, 4, 9,
        1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
    ],
    [
        15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10, 3, 13, 4, 7, 15, 2, 8, 14, 12, 0, 1,
        10, 6, 9, 11, 5, 0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15, 13, 8, 10, 1, 3, 15,
        4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
    ],
    [
        10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8, 13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5,
        14, 12, 11, 15, 1, 13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7, 1, 10, 13, 0, 6,
        9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
    ],
    [
        7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15, 13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2,
        12, 1, 10, 14, 9, 10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4, 3, 15, 0, 6, 10, 1,
        13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
    ],
    [
        2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9, 14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15,
        10, 3, 9, 8, 6, 4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14, 11, 8, 12, 7, 1, 14,
        2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
    ],
    [
        12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11, 10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13,
        14, 0, 11, 3, 8, 9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6, 4, 3, 2, 12, 9, 5,
        15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
    ],
    [
        4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1, 13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5,
        12, 2, 15, 8, 6, 1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2, 6, 11, 13, 8, 1, 4,
        10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
    ],
    [
        13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7, 1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6,
        11, 0, 14, 9, 2, 7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8, 2, 1, 14, 7, 4, 10,
        8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
    ],
];

/// Picks bits of `input` (`width` wide, 1 the most significant) in the order `table` names them.
fn permute(input: u64, width: u32, table: &[u8]) -> u64 {
    table.iter().fold(0, |out, &position| {
        out << 1 | (input >> (width - u32::from(position))) & 1
    })
}

fn rotate28(half: u64, by: u32) -> u64 {
    (half << by | half >> (28 - by)) & 0x0FFF_FFFF
}

fn feistel(right: u64, subkey: u64) -> u64 {
    let mixed = permute(right, 32, &EXPANSION) ^ subkey;
    let substituted = SBOXES.iter().enumerate().fold(0, |out, (index, sbox)| {
        let six = (mixed >> (42 - 6 * index)) & 0x3F;
        let row = (six >> 4 & 0b10) | (six & 1);
        let column = six >> 1 & 0xF;
        out << 4 | u64::from(sbox[(row * 16 + column) as usize])
    });
    permute(substituted, 32, &PERMUTATION)
}

impl Des {
    /// Keys with an 8-byte key; the parity bits are ignored as the standard says.
    pub(crate) fn new(key: [u8; 8]) -> Self {
        let chosen = permute(u64::from_be_bytes(key), 64, &CHOICE_ONE);
        let (mut left, mut right) = (chosen >> 28, chosen & 0x0FFF_FFFF);
        let mut subkeys = [0; ROUNDS];
        for (subkey, shift) in subkeys.iter_mut().zip(SHIFTS) {
            left = rotate28(left, shift);
            right = rotate28(right, shift);
            *subkey = permute(left << 28 | right, 56, &CHOICE_TWO);
        }
        Self { subkeys }
    }

    pub(crate) fn encrypt(&self, block: &mut [u8; 8]) {
        let permuted = permute(u64::from_be_bytes(*block), 64, &INITIAL);
        let (mut left, mut right) = (permuted >> 32, permuted & 0xFFFF_FFFF);
        for subkey in self.subkeys {
            (left, right) = (right, left ^ feistel(right, subkey));
        }
        *block = permute(right << 32 | left, 64, &FINAL).to_be_bytes();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encrypt(key: u64, plain: u64) -> u64 {
        let mut block = plain.to_be_bytes();
        Des::new(key.to_be_bytes()).encrypt(&mut block);
        u64::from_be_bytes(block)
    }

    #[test]
    fn the_classic_worked_example_encrypts() {
        assert_eq!(
            encrypt(0x1334_5779_9BBC_DFF1, 0x0123_4567_89AB_CDEF),
            0x85E8_1354_0F0A_B405
        );
    }

    #[test]
    fn nist_sp_800_17_variable_plaintext_vectors_encrypt() {
        assert_eq!(
            encrypt(0x0101_0101_0101_0101, 0x8000_0000_0000_0000),
            0x95F8_A5E5_DD31_D900
        );
        assert_eq!(
            encrypt(0x0101_0101_0101_0101, 0x0000_0000_0000_0001),
            0x166B_40B4_4ABA_4BD6
        );
    }

    #[test]
    fn nist_sp_800_17_variable_key_vector_encrypts() {
        assert_eq!(
            encrypt(0x8001_0101_0101_0101, 0x0000_0000_0000_0000),
            0x95A8_D728_13DA_A94D
        );
    }
}

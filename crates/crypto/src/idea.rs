//! IDEA with little-endian words, as Swissphone `DE9xx` pagers run it.

pub const IDEA_KEY_BYTES: usize = 16;
pub const IDEA_BLOCK_BYTES: usize = 8;
const ROUNDS: usize = 8;
const KEY_ROTATION_BITS: u32 = 25;

fn mul(a: u16, b: u16) -> u16 {
    let product = u32::from(a) * u32::from(b);
    if product == 0 {
        return 1u16.wrapping_sub(a).wrapping_sub(b);
    }
    let (low, high) = (product as u16, (product >> 16) as u16);
    low.wrapping_sub(high).wrapping_add(u16::from(low < high))
}

struct Subkeys {
    key: [u8; IDEA_KEY_BYTES],
    at: usize,
}

impl Subkeys {
    fn new(key: &[u8; IDEA_KEY_BYTES]) -> Self {
        Self { key: *key, at: 0 }
    }

    fn next(&mut self) -> u16 {
        let word = u16::from_le_bytes([self.key[self.at], self.key[self.at + 1]]);
        self.at += 2;
        if self.at == IDEA_KEY_BYTES {
            self.key = u128::from_be_bytes(self.key)
                .rotate_left(KEY_ROTATION_BITS)
                .to_be_bytes();
            self.at = 0;
        }
        word
    }
}

fn words(block: [u8; IDEA_BLOCK_BYTES]) -> [u16; 4] {
    std::array::from_fn(|i| u16::from_le_bytes([block[2 * i], block[2 * i + 1]]))
}

fn bytes(words: [u16; 4]) -> [u8; IDEA_BLOCK_BYTES] {
    std::array::from_fn(|i| words[i / 2].to_le_bytes()[i % 2])
}

/// One block through IDEA, its 16-bit words read little-endian.
#[must_use]
pub fn idea_encrypt_le(
    key: &[u8; IDEA_KEY_BYTES],
    block: [u8; IDEA_BLOCK_BYTES],
) -> [u8; IDEA_BLOCK_BYTES] {
    let mut keys = Subkeys::new(key);
    let [mut x1, mut x2, mut x3, mut x4] = words(block);
    for _ in 0..ROUNDS {
        x1 = mul(x1, keys.next());
        x2 = x2.wrapping_add(keys.next());
        x3 = x3.wrapping_add(keys.next());
        x4 = mul(x4, keys.next());
        let t0 = mul(x1 ^ x3, keys.next());
        let t1 = mul((x2 ^ x4).wrapping_add(t0), keys.next());
        let t2 = t0.wrapping_add(t1);
        x1 ^= t1;
        x4 = x1 ^ t2;
        let y2 = x2;
        x2 = x3 ^ t1;
        x3 = y2 ^ t2;
    }
    let y1 = mul(x1, keys.next());
    let y2 = x3.wrapping_add(keys.next());
    let y3 = x2.wrapping_add(keys.next());
    let y4 = mul(x4, keys.next());
    bytes([y1, y2, y3, y4])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex;

    #[test]
    fn multiplication_treats_zero_as_two_to_the_sixteenth() {
        assert_eq!(mul(0, 0), 1);
        assert_eq!(mul(0, 5), 65_532);
        assert_eq!(mul(5, 0), 65_532);
        assert_eq!(mul(3, 4), 12);
        assert_eq!(mul(0xFFFF, 0xFFFF), 4);
        assert_eq!(mul(0x8000, 2), 0);
    }

    #[test]
    fn block_cipher_matches_the_pager_firmware_vectors() {
        let key = std::array::from_fn(|i| i as u8);
        let block = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        assert_eq!(
            Some(idea_encrypt_le(&key, block).to_vec()),
            hex("be58cec0043639c6")
        );
        assert_eq!(
            Some(idea_encrypt_le(&[0; IDEA_KEY_BYTES], [0; 8]).to_vec()),
            hex("f8fffaff0f00f3ff")
        );
    }
}

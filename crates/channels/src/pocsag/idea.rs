//! The message cipher of Swissphone DE9xx pagers: IDEA with little-endian words, run as a
//! keystream over the alpha characters. Transcribed from the DE920 firmware.
use std::time::SystemTime;

pub(crate) const KEY_BYTES: usize = 16;
const BLOCK_BYTES: usize = 8;
const ROUNDS: usize = 8;
const KEY_ROTATION_BITS: u32 = 25;
const WORD_BITS: usize = 20;
#[cfg(any(test, feature = "synth"))]
const WORD_MASK: u32 = (1 << WORD_BITS) - 1;
pub(crate) const HEADER_BITS: usize = 2 * WORD_BITS;
const LFSR_TAPS: u32 = 0x57;
const LFSR_CLOCKS: usize = 65;
const CHAR_MASK: u8 = 0x7F;
const YEAR_OFFSET: u8 = 90;
const NO_TIME: u8 = 0xF;
const MINUTE_STEP: u8 = 5;
const LATE_MINUTE: u8 = 20;
const BACKWARD_SLACK: u8 = 21;
const FORWARD_SLACK: u8 = 40;

fn mul(a: u16, b: u16) -> u16 {
    let product = u32::from(a) * u32::from(b);
    if product == 0 {
        return 1u16.wrapping_sub(a).wrapping_sub(b);
    }
    let (low, high) = (product as u16, (product >> 16) as u16);
    low.wrapping_sub(high).wrapping_add(u16::from(low < high))
}

struct Subkeys {
    key: [u8; KEY_BYTES],
    at: usize,
}

impl Subkeys {
    fn new(key: &[u8; KEY_BYTES]) -> Self {
        Self { key: *key, at: 0 }
    }

    fn next(&mut self) -> u16 {
        let word = u16::from_le_bytes([self.key[self.at], self.key[self.at + 1]]);
        self.at += 2;
        if self.at == KEY_BYTES {
            self.key = u128::from_be_bytes(self.key)
                .rotate_left(KEY_ROTATION_BITS)
                .to_be_bytes();
            self.at = 0;
        }
        word
    }
}

fn words(block: [u8; BLOCK_BYTES]) -> [u16; 4] {
    std::array::from_fn(|i| u16::from_le_bytes([block[2 * i], block[2 * i + 1]]))
}

fn bytes(words: [u16; 4]) -> [u8; BLOCK_BYTES] {
    std::array::from_fn(|i| words[i / 2].to_le_bytes()[i % 2])
}

pub(crate) fn encrypt_block(key: &[u8; KEY_BYTES], block: [u8; BLOCK_BYTES]) -> [u8; BLOCK_BYTES] {
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

pub(crate) struct Keystream {
    key: [u8; KEY_BYTES],
    lfsr: u32,
    block: [u8; BLOCK_BYTES],
    left: usize,
}

impl Keystream {
    pub(crate) fn new(key: &[u8; KEY_BYTES], seed: [u8; 4]) -> Self {
        Self {
            key: *key,
            lfsr: u32::from_be_bytes(seed),
            block: [0; BLOCK_BYTES],
            left: 0,
        }
    }

    fn refill(&mut self) {
        let mut carry = self.lfsr >> 31 != 0;
        let mut bits = u64::from_be_bytes(self.block);
        for _ in 0..LFSR_CLOCKS {
            if carry {
                self.lfsr ^= LFSR_TAPS;
            }
            let out = self.lfsr >> 31 != 0;
            self.lfsr = self.lfsr << 1 | u32::from(carry);
            bits = bits << 1 | u64::from(out);
            carry = out;
        }
        self.block = encrypt_block(&self.key, bits.to_be_bytes());
        self.left = BLOCK_BYTES;
    }

    pub(crate) fn mask(&mut self) -> u8 {
        if self.left == 0 {
            self.refill();
        }
        self.left -= 1;
        self.block[self.left] & CHAR_MASK
    }
}

/// The pager's own calendar: a two-digit year and the leap rule of its firmware.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Civil {
    pub year: u8,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
}

impl Civil {
    pub(crate) fn local(at: SystemTime) -> Self {
        let zoned = jiff::Timestamp::try_from(at)
            .unwrap_or_default()
            .to_zoned(jiff::tz::TimeZone::system());
        let field = |value: i64| u8::try_from(value).unwrap_or(0);
        Self {
            year: field(i64::from(zoned.year()).rem_euclid(100)),
            month: field(i64::from(zoned.month())),
            day: field(i64::from(zoned.day())),
            hour: field(i64::from(zoned.hour())),
            minute: field(i64::from(zoned.minute())),
        }
    }

    fn days_in_month(year: u8, month: u8) -> u8 {
        match month {
            2 if year.is_multiple_of(4) => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        }
    }

    pub(crate) fn later_hour(self) -> Self {
        let mut next = self;
        next.hour = (self.hour + 1) % 24;
        if next.hour != 0 {
            return next;
        }
        next.day = self.day % Self::days_in_month(self.year, self.month) + 1;
        if next.day != 1 {
            return next;
        }
        next.month = self.month % 12 + 1;
        if next.month == 1 {
            next.year = (self.year + 1) % 100;
        }
        next
    }

    pub(crate) fn earlier_hour(self) -> Self {
        let mut prior = self;
        if self.hour > 0 {
            prior.hour = self.hour - 1;
            return prior;
        }
        prior.hour = 23;
        if self.day > 1 {
            prior.day = self.day - 1;
            return prior;
        }
        prior.month = if self.month > 1 { self.month - 1 } else { 12 };
        if prior.month == 12 {
            prior.year = if self.year > 0 { self.year - 1 } else { 99 };
        }
        prior.day = Self::days_in_month(prior.year, prior.month);
        prior
    }
}

pub(crate) fn key_sum(key: &[u8; KEY_BYTES]) -> u8 {
    key.iter().fold(0, |acc, &byte| acc.wrapping_add(byte))
}

/// The two 20-bit codewords that open an encrypted message. They mask the seed and carry the
/// minute the message was sent in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Header {
    pub(crate) first: u32,
    pub(crate) second: u32,
}

impl Header {
    pub(crate) fn parse(bits: &[bool]) -> Option<(Self, &[bool])> {
        if bits.len() < HEADER_BITS {
            return None;
        }
        let word = |chunk: &[bool]| chunk.iter().fold(0, |acc, &bit| acc << 1 | u32::from(bit));
        let header = Self {
            first: word(&bits[..WORD_BITS]),
            second: word(&bits[WORD_BITS..HEADER_BITS]),
        };
        Some((header, &bits[HEADER_BITS..]))
    }

    pub(crate) fn bits(&self) -> Vec<bool> {
        [self.first, self.second]
            .into_iter()
            .flat_map(|word| (0..WORD_BITS).rev().map(move |i| word >> i & 1 == 1))
            .collect()
    }

    /// A header that names `minute` under `key`, with `mask` as its first word.
    #[cfg(any(test, feature = "synth"))]
    pub(crate) fn sealing(mask: u32, key: &[u8; KEY_BYTES], minute: u8) -> Self {
        let first = mask & WORD_MASK;
        let nibble = minute / MINUTE_STEP;
        Self {
            first,
            second: u32::from(Self::masks(first)[0] ^ key_sum(key) ^ nibble) << 12,
        }
    }

    fn masks(word: u32) -> [u8; 3] {
        [(word >> 12) as u8, (word >> 4) as u8, (word as u8) << 4]
    }

    pub(crate) fn minute_nibble(&self, key: &[u8; KEY_BYTES]) -> u8 {
        (Self::masks(self.first)[0] ^ Self::masks(self.second)[0] ^ key_sum(key)) & 0xF
    }

    fn sent_at(nibble: u8, clock: Civil) -> Civil {
        if nibble == NO_TIME {
            return Civil {
                year: 0,
                month: 0,
                day: 0,
                hour: 0,
                minute: 0,
            };
        }
        let minute = nibble * MINUTE_STEP;
        if minute >= LATE_MINUTE {
            if clock.minute + BACKWARD_SLACK <= minute {
                return clock.earlier_hour();
            }
        } else if clock.minute >= minute + FORWARD_SLACK {
            return clock.later_hour();
        }
        clock
    }

    pub(crate) fn seed(&self, key: &[u8; KEY_BYTES], org_code: u16, clock: Civil) -> [u8; 4] {
        let nibble = self.minute_nibble(key);
        let [year_mask, day_mask, hour_mask] = Self::masks(self.first);
        let [org_low, org_high] = org_code.to_le_bytes();
        let sent = Self::sent_at(nibble, clock);
        [
            sent.year.wrapping_sub(YEAR_OFFSET) ^ year_mask,
            sent.day ^ day_mask,
            sent.hour ^ (hour_mask | org_high),
            (sent.month << 4 | nibble) ^ org_low,
        ]
    }
}

/// The characters of a message up to its terminator. A wrong key gives noise, never an error:
/// the air format carries nothing a receiver could check without the pager's own tables.
pub(crate) fn decrypt(
    key: &[u8; KEY_BYTES],
    org_code: u16,
    clock: Civil,
    header: &Header,
    sealed: &[u8],
) -> Vec<u8> {
    let mut stream = Keystream::new(key, header.seed(key, org_code, clock));
    sealed
        .iter()
        .map(|&byte| byte ^ stream.mask())
        .take_while(|&byte| byte != 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> [u8; KEY_BYTES] {
        std::array::from_fn(|i| i as u8)
    }

    fn hex(text: &str) -> Vec<u8> {
        crate::crypto::hex(text).unwrap()
    }

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
        let block = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        assert_eq!(
            encrypt_block(&key(), block).to_vec(),
            hex("be58cec0043639c6")
        );
        assert_eq!(
            encrypt_block(&[0; KEY_BYTES], [0; 8]).to_vec(),
            hex("f8fffaff0f00f3ff")
        );
    }

    #[test]
    fn keystream_matches_the_pager_firmware_vector() {
        let mut stream = Keystream::new(&key(), [0xC0, 0x09, 0x0D, 0xA3]);
        let masks: Vec<u8> = (0..20).map(|_| stream.mask()).collect();
        assert_eq!(masks, hex("5a170051016c716e1155294b17143d413f184b35"));
    }

    const CLOCK: Civil = Civil {
        year: 26,
        month: 10,
        day: 9,
        hour: 13,
        minute: 17,
    };

    fn header(nibble: u8) -> Header {
        let sum = key_sum(&key());
        Header {
            first: 0x12345,
            second: u32::from(0x12 ^ sum ^ nibble) << 12,
        }
    }

    #[test]
    fn seed_mixes_date_hour_and_header_masks() {
        assert_eq!(header(3).minute_nibble(&key()), 3);
        assert_eq!(
            header(3).seed(&key(), 0x4af5, CLOCK),
            [0xC0 ^ 0x12, 9 ^ 0x34, 13 ^ (0x50 | 0x4a), 0xA3 ^ 0xF5]
        );
    }

    #[test]
    fn minute_nibble_fifteen_zeroes_the_date() {
        assert_eq!(
            header(15).seed(&key(), 0x4af5, CLOCK),
            [0u8.wrapping_sub(90) ^ 0x12, 0x34, 0x50 | 0x4a, 0x0F ^ 0xF5]
        );
    }

    #[test]
    fn the_hour_follows_the_minute_in_the_header_across_the_hour_mark() {
        let at = |minute| Civil { minute, ..CLOCK };
        let hour = |nibble: u8, minute| header(nibble).seed(&key(), 0, at(minute))[2] ^ 0x50;
        assert_eq!(hour(11, 10), 12, "sent at :55, received at :10");
        assert_eq!(hour(11, 40), 13, "sent at :55, received at :40");
        assert_eq!(hour(1, 50), 14, "sent at :05, received at :50");
        assert_eq!(hour(1, 40), 13, "sent at :05, received at :40");
        assert_eq!(hour(6, 25), 13);
    }

    #[test]
    fn civil_time_steps_across_day_month_and_year_edges() {
        let civil = |year, month, day, hour| Civil {
            year,
            month,
            day,
            hour,
            minute: 0,
        };
        assert_eq!(civil(99, 12, 31, 23).later_hour(), civil(0, 1, 1, 0));
        assert_eq!(civil(24, 3, 1, 0).earlier_hour(), civil(24, 2, 29, 23));
        assert_eq!(civil(26, 3, 1, 0).earlier_hour(), civil(26, 2, 28, 23));
        assert_eq!(civil(0, 1, 1, 0).earlier_hour(), civil(99, 12, 31, 23));
        assert_eq!(civil(26, 4, 30, 23).later_hour(), civil(26, 5, 1, 0));
    }

    #[test]
    fn local_time_reads_the_host_zone() {
        let at = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000);
        let civil = Civil::local(at);
        assert_eq!(civil.year, 27);
        assert_eq!(civil.month, 1);
        assert!((14..=16).contains(&civil.day), "{civil:?}");
    }

    #[test]
    fn decrypt_stops_at_the_first_nul() {
        let seed = header(3).seed(&key(), 0x4af5, CLOCK);
        let mut stream = Keystream::new(&key(), seed);
        let sealed: Vec<u8> = b"HELLO\0junk"
            .iter()
            .map(|&byte| byte ^ stream.mask())
            .collect();
        let text = decrypt(&key(), 0x4af5, CLOCK, &header(3), &sealed);
        assert_eq!(text, b"HELLO");
    }

    #[test]
    fn a_header_needs_forty_bits() {
        let bits: Vec<bool> = (0..45).map(|i| i % 3 == 0).collect();
        assert!(Header::parse(&bits[..39]).is_none());
        let (header, rest) = Header::parse(&bits).unwrap();
        assert_eq!(rest.len(), 5);
        assert_eq!(header.bits(), bits[..40]);
    }
}

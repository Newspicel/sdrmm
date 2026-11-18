//! The message cipher of Swissphone `DE9xx` pagers: IDEA with little-endian words, run as a
//! keystream over the alpha characters. Transcribed from the DE920 firmware.
use std::time::SystemTime;

use sdrmm_crypto::{IDEA_BLOCK_BYTES, IDEA_KEY_BYTES, idea_encrypt_le};

pub(crate) const KEY_BYTES: usize = IDEA_KEY_BYTES;
const BLOCK_BYTES: usize = IDEA_BLOCK_BYTES;
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
/// The byte sums a pager folds into the second header word: that of its built-in key once it
/// has loaded its programming, zero before. They set the check bits and mask the minute nibble;
/// no operator key takes part.
const PAGER_SUMS: [u8; 2] = [0x4B, 0];
#[cfg(any(test, feature = "synth"))]
const SEALING_SUM: u8 = PAGER_SUMS[0];

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
        self.block = idea_encrypt_le(&self.key, bits.to_be_bytes());
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

/// The two bytes a pager with `sum` expects behind the day and hour masks.
fn check_bytes(sum: u8) -> (u8, u8) {
    let mid = sum.wrapping_mul(2).wrapping_add(3);
    let low = mid.wrapping_mul(2).wrapping_add(3) & 0xF;
    (mid, low)
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

    #[cfg(any(test, feature = "synth"))]
    pub(crate) fn bits(self) -> Vec<bool> {
        [self.first, self.second]
            .into_iter()
            .flat_map(|word| (0..WORD_BITS).rev().map(move |i| word >> i & 1 == 1))
            .collect()
    }

    /// A header that names `minute`, with `mask` as its first word, as a programmed pager
    /// checks it.
    #[cfg(any(test, feature = "synth"))]
    pub(crate) fn sealing(mask: u32, minute: u8) -> Self {
        let first = mask & WORD_MASK;
        let nibble = minute / MINUTE_STEP;
        let [top, mid, low] = Self::fields(first);
        let (check_mid, check_low) = check_bytes(SEALING_SUM);
        Self {
            first,
            second: u32::from(top ^ SEALING_SUM ^ nibble) << 12
                | u32::from(mid ^ check_mid) << 4
                | u32::from(low ^ check_low),
        }
    }

    /// Bits 12-19, 4-11 and 0-3 of a word.
    fn fields(word: u32) -> [u8; 3] {
        [(word >> 12) as u8, (word >> 4) as u8, (word & 0xF) as u8]
    }

    /// The pager sum whose check bits this header carries, if any.
    pub(crate) fn pager_sum(self) -> Option<u8> {
        PAGER_SUMS.into_iter().find(|&sum| self.passes(sum))
    }

    fn passes(self, sum: u8) -> bool {
        let [_, day_mask, hour_mask] = Self::fields(self.first);
        let [_, mid, low] = Self::fields(self.second);
        (mid ^ day_mask, low ^ hour_mask) == check_bytes(sum)
    }

    pub(crate) fn minute_nibble(self, sum: u8) -> u8 {
        (Self::fields(self.first)[0] ^ Self::fields(self.second)[0] ^ sum) & 0xF
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

    pub(crate) fn seed(self, sum: u8, org_code: u16, clock: Civil) -> [u8; 4] {
        let nibble = self.minute_nibble(sum);
        let [year_mask, day_mask, hour_mask] = Self::fields(self.first);
        let [org_low, org_high] = org_code.to_le_bytes();
        let sent = Self::sent_at(nibble, clock);
        [
            sent.year.wrapping_sub(YEAR_OFFSET) ^ year_mask,
            sent.day ^ day_mask,
            sent.hour ^ (hour_mask << 4 | org_high),
            (sent.month << 4 | nibble) ^ org_low,
        ]
    }
}

/// The characters of a message up to its terminator, or `None` when the header fails the
/// pager's check. The check involves no key, so a wrong key gives noise, never an error.
pub(crate) fn decrypt(
    key: &[u8; KEY_BYTES],
    org_code: u16,
    clock: Civil,
    header: Header,
    sealed: &[u8],
) -> Option<Vec<u8>> {
    let sum = header.pager_sum()?;
    let mut stream = Keystream::new(key, header.seed(sum, org_code, clock));
    let text = sealed
        .iter()
        .map(|&byte| byte ^ stream.mask())
        .take_while(|&byte| byte != 0)
        .collect();
    Some(text)
}

/// `text`, terminator included, under the keystream of a header made by `Header::sealing`.
#[cfg(any(test, feature = "synth"))]
pub(crate) fn seal(
    key: &[u8; KEY_BYTES],
    org_code: u16,
    clock: Civil,
    header: Header,
    text: &[u8],
) -> Vec<u8> {
    let mut stream = Keystream::new(key, header.seed(SEALING_SUM, org_code, clock));
    text.iter().map(|&byte| byte ^ stream.mask()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> [u8; KEY_BYTES] {
        std::array::from_fn(|i| i as u8)
    }

    fn hex(text: &str) -> Vec<u8> {
        sdrmm_crypto::hex(text).unwrap()
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

    const MASK: u32 = 0x12345;

    fn header(nibble: u8) -> Header {
        Header::sealing(MASK, nibble * MINUTE_STEP)
    }

    #[test]
    fn a_sealed_header_carries_the_check_bits_of_the_pager_firmware() {
        let second = header(3).second;
        assert_eq!(
            second >> 12,
            0x12 ^ 0x4B ^ 3,
            "top byte: year mask, sum, minute"
        );
        assert_eq!(
            second >> 4 & 0xFF,
            0x34 ^ 0x99,
            "mid byte: day mask, 2 * sum + 3"
        );
        assert_eq!(second & 0xF, 5 ^ 5, "low nibble: hour mask, 4 * sum + 9");
    }

    #[test]
    fn a_header_names_the_pager_sum_its_check_bits_hold() {
        assert_eq!(header(3).pager_sum(), Some(0x4B));
        let fresh_pager = Header {
            first: MASK,
            second: (0x12 ^ 3) << 12 | (0x34 ^ 0x03) << 4 | (5 ^ 9),
        };
        assert_eq!(fresh_pager.pager_sum(), Some(0));
        assert_eq!(fresh_pager.minute_nibble(0), 3);
        let flipped = Header {
            second: header(3).second ^ 1 << 7,
            ..header(3)
        };
        assert_eq!(flipped.pager_sum(), None);
    }

    #[test]
    fn seed_mixes_date_hour_and_header_masks() {
        assert_eq!(header(3).minute_nibble(0x4B), 3);
        assert_eq!(
            header(3).seed(0x4B, 0x4af5, CLOCK),
            [0xC0 ^ 0x12, 0x09 ^ 0x34, 0x0D ^ (0x50 | 0x4a), 0xA3 ^ 0xF5]
        );
    }

    #[test]
    fn minute_nibble_fifteen_zeroes_the_date() {
        assert_eq!(
            header(15).seed(0x4B, 0x4af5, CLOCK),
            [0u8.wrapping_sub(90) ^ 0x12, 0x34, 0x50 | 0x4a, 0x0F ^ 0xF5]
        );
    }

    #[test]
    fn the_hour_follows_the_minute_in_the_header_across_the_hour_mark() {
        let at = |minute| Civil { minute, ..CLOCK };
        let hour = |nibble: u8, minute| header(nibble).seed(0x4B, 0, at(minute))[2] ^ 0x50;
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
        let at = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_hours(500000);
        let civil = Civil::local(at);
        assert_eq!(civil.year, 27);
        assert_eq!(civil.month, 1);
        assert!((14..=16).contains(&civil.day), "{civil:?}");
    }

    #[test]
    fn decrypt_stops_at_the_first_nul() {
        let sealed = seal(&key(), 0x4af5, CLOCK, header(3), b"HELLO\0junk");
        let text = decrypt(&key(), 0x4af5, CLOCK, header(3), &sealed);
        assert_eq!(text.as_deref(), Some(&b"HELLO"[..]));
    }

    #[test]
    fn decrypt_rejects_a_header_that_fails_the_check() {
        let sealed = seal(&key(), 0x4af5, CLOCK, header(3), b"HELLO\0");
        let damaged = Header {
            second: header(3).second ^ 1,
            ..header(3)
        };
        assert_eq!(decrypt(&key(), 0x4af5, CLOCK, damaged, &sealed), None);
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

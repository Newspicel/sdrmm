const LIST_OCTETS: usize = 16;
const ARFCN_SPACE: u16 = 1024;
const BITMAP_ZERO_HIGHEST: u16 = 124;
const VARIABLE_BITS: u16 = 111;
const MAX_W: usize = 29;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    BitmapZero,
    Range(u16),
    Variable,
}

fn format(first: u8) -> Option<Format> {
    match (first >> 6, first & 0x08 != 0, first >> 1 & 3) {
        (0, _, _) => Some(Format::BitmapZero),
        (2, false, _) => Some(Format::Range(1024)),
        (2, true, 0) => Some(Format::Range(512)),
        (2, true, 1) => Some(Format::Range(256)),
        (2, true, 2) => Some(Format::Range(128)),
        (2, true, 3) => Some(Format::Variable),
        _ => None,
    }
}

struct Bits<'a> {
    octets: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn take(&mut self, width: u32) -> Option<u16> {
        let mut value = 0u16;
        for _ in 0..width {
            let octet = self.octets.get(self.at / 8)?;
            value = value << 1 | u16::from(octet >> (7 - self.at % 8) & 1);
            self.at += 1;
        }
        Some(value)
    }
}

pub(crate) fn decode(octets: &[u8], out: &mut Vec<u16>) {
    let octets = &octets[..octets.len().min(LIST_OCTETS)];
    let Some(&first) = octets.first() else {
        return;
    };
    match format(first) {
        Some(Format::BitmapZero) => bitmap_zero(octets, out),
        Some(Format::Range(range)) => ranged(octets, range, out),
        Some(Format::Variable) => variable(octets, out),
        None => {}
    }
    out.sort_unstable();
    out.dedup();
}

fn bitmap_zero(octets: &[u8], out: &mut Vec<u16>) {
    for (k, &octet) in octets.iter().enumerate() {
        for bit in 0..8u16 {
            let arfcn = (LIST_OCTETS - 1 - k) as u16 * 8 + bit + 1;
            if arfcn <= BITMAP_ZERO_HIGHEST && octet >> bit & 1 == 1 {
                out.push(arfcn);
            }
        }
    }
}

fn variable(octets: &[u8], out: &mut Vec<u16>) {
    let mut bits = Bits { octets, at: 7 };
    let Some(origin) = bits.take(10) else {
        return;
    };
    out.push(origin);
    for relative in 1..=VARIABLE_BITS {
        match bits.take(1) {
            Some(1) => out.push((origin + relative) % ARFCN_SPACE),
            Some(_) => {}
            None => break,
        }
    }
}

fn ranged(octets: &[u8], range: u16, out: &mut Vec<u16>) {
    let (mut bits, origin) = if range == 1024 {
        if octets[0] & 0x04 != 0 {
            out.push(0);
        }
        (Bits { octets, at: 6 }, 0)
    } else {
        let mut bits = Bits { octets, at: 7 };
        let Some(origin) = bits.take(10) else {
            return;
        };
        out.push(origin);
        (bits, origin)
    };
    let top = range.trailing_zeros();
    let mut w = [0u16; MAX_W + 1];
    let mut count = 0;
    for (k, slot) in w.iter_mut().enumerate().skip(1) {
        let depth = usize::BITS - 1 - k.leading_zeros();
        let Some(width) = top.checked_sub(depth).filter(|&width| width > 0) else {
            break;
        };
        match bits.take(width) {
            Some(0) | None => break,
            Some(value) => {
                *slot = value;
                count = k;
            }
        }
    }
    for k in 1..=count {
        let value = unfold(&w, k, i32::from(range));
        out.push(if range == 1024 {
            value as u16
        } else {
            (origin + value as u16) % ARFCN_SPACE
        });
    }
}

fn unfold(w: &[u16; MAX_W + 1], k: usize, range: i32) -> i32 {
    let mut index = k;
    let mut j = 1usize << (usize::BITS - 1 - k.leading_zeros());
    let mut n = i32::from(w[index]);
    while index > 1 {
        let span = range / j as i32;
        let modulus = 2 * span - 1;
        if 2 * index < 3 * j {
            index -= j / 2;
            n = (n + i32::from(w[index]) - span - 1).rem_euclid(modulus) + 1;
        } else {
            index -= j;
            n = (n + i32::from(w[index]) - 1).rem_euclid(modulus) + 1;
        }
        j /= 2;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(octets: &[u8]) -> Vec<u16> {
        let mut out = Vec::new();
        decode(octets, &mut out);
        out
    }

    #[test]
    fn the_captured_variable_bitmaps_decode() {
        let si1 = [0x8F, 0x6A, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(list(&si1), [725]);
        let si2 = [
            0x8F, 0x6D, 0x18, 0x10, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        assert_eq!(list(&si2), [730, 733, 734, 741, 746]);
    }

    #[test]
    fn bitmap_zero_counts_from_the_last_octet() {
        let mut octets = [0u8; 16];
        octets[15] = 0b0000_0001;
        octets[14] = 0b1000_0000;
        octets[0] = 0b0000_1000;
        assert_eq!(list(&octets), [1, 16, 124]);
    }

    fn range_1024(f0: bool, w: &[u16]) -> [u8; 16] {
        let mut octets = [0u8; 16];
        octets[0] = 0x80 | if f0 { 0x04 } else { 0 };
        let mut at = 6;
        for (k, &value) in w.iter().enumerate() {
            let index = k + 1;
            let depth = (usize::BITS - 1 - index.leading_zeros()) as usize;
            let width = 10 - depth;
            for bit in (0..width).rev() {
                if value >> bit & 1 == 1 {
                    octets[at / 8] |= 0x80 >> (at % 8);
                }
                at += 1;
            }
        }
        octets
    }

    #[test]
    fn range_1024_follows_the_closed_forms() {
        let (w1, w2, w3, w4, w5) = (700u16, 300u16, 200u16, 100u16, 50u16);
        let f2 = (i32::from(w1) - 512 + i32::from(w2) - 1).rem_euclid(1023) + 1;
        let f3 = (i32::from(w1) + i32::from(w3) - 1).rem_euclid(1023) + 1;
        let f4 = (i32::from(w1) - 512
            + ((i32::from(w2) - 256 + i32::from(w4) - 1).rem_euclid(511)))
        .rem_euclid(1023)
            + 1;
        let f5 = (i32::from(w1) + ((i32::from(w3) - 256 + i32::from(w5) - 1).rem_euclid(511)))
            .rem_euclid(1023)
            + 1;
        let mut expected: Vec<u16> = [0, i32::from(w1), f2, f3, f4, f5]
            .iter()
            .map(|&f| f as u16)
            .collect();
        expected.sort_unstable();
        assert_eq!(list(&range_1024(true, &[w1, w2, w3, w4, w5])), expected);
    }

    #[test]
    fn a_single_channel_range_1024_list_is_its_first_word() {
        assert_eq!(list(&range_1024(false, &[17])), [17]);
    }
}

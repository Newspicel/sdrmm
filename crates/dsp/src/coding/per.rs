//! ASN.1 unaligned PER (X.691) over a packed bit stream.

use super::bits::BitReader;

pub struct Per<'a> {
    bits: BitReader<'a>,
}

impl<'a> Per<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bits: BitReader::new(bytes),
        }
    }

    /// Ends the stream `bits` in, when it holds that many.
    pub fn limit(&mut self, bits: usize) {
        self.bits.limit(bits);
    }

    pub fn bit(&mut self) -> Option<u8> {
        self.bits.bit().map(u8::from)
    }

    pub fn uint(&mut self, n: usize) -> Option<u64> {
        if self.bits.remaining() < n {
            return None;
        }
        let mut value = 0u64;
        for _ in 0..n {
            value = (value << 1) | u64::from(self.bits.bit()?);
        }
        Some(value)
    }

    pub fn constrained(&mut self, lo: i64, hi: i64) -> Option<i64> {
        let range = (hi - lo + 1) as u64;
        if range == 1 {
            return Some(lo);
        }
        let bits = 64 - (range - 1).leading_zeros() as usize;
        Some(lo + self.uint(bits)? as i64)
    }

    fn length_frag(&mut self) -> Option<(usize, bool)> {
        if self.bit()? == 0 {
            return Some((self.uint(7)? as usize, false));
        }
        if self.bit()? == 0 {
            return Some((self.uint(14)? as usize, false));
        }
        let m = self.uint(6)? as usize;
        if m == 0 || m > 4 {
            return None;
        }
        Some((m * 16384, true))
    }

    pub fn length(&mut self) -> Option<usize> {
        let (mut total, mut more) = self.length_frag()?;
        while more {
            let (n, m) = self.length_frag()?;
            total = total.checked_add(n)?;
            more = m;
        }
        Some(total)
    }

    pub fn normally_small(&mut self) -> Option<u64> {
        if self.bit()? == 0 {
            return self.uint(6);
        }
        let n = self.length()?;
        let bytes = self.remaining_bytes(n * 8)?;
        Some(bytes.iter().fold(0u64, |v, &b| (v << 8) | u64::from(b)))
    }

    pub fn ia5(&mut self, min: i64, max: i64) -> Option<String> {
        let n = self.constrained(min, max)? as usize;
        let mut s = String::with_capacity(n);
        for _ in 0..n {
            s.push(self.uint(7)? as u8 as char);
        }
        Some(s)
    }

    /// A string of `min..=max` characters from `alphabet`, which lists the permitted
    /// characters in ascending order. Each character takes the fewest bits that count the
    /// alphabet, and is sent as itself when that fits, as its index otherwise (X.691 30.5.4).
    pub fn restricted(&mut self, min: i64, max: i64, alphabet: &[u8]) -> Option<String> {
        let n = self.constrained(min, max)? as usize;
        let bits = usize::BITS as usize - alphabet.len().saturating_sub(1).leading_zeros() as usize;
        let highest = alphabet.last().copied()?;
        let as_itself = usize::from(highest) < 1 << bits;
        let mut s = String::with_capacity(n);
        for _ in 0..n {
            let value = self.uint(bits)? as usize;
            let byte = if as_itself {
                u8::try_from(value)
                    .ok()
                    .filter(|byte| alphabet.contains(byte))?
            } else {
                *alphabet.get(value)?
            };
            s.push(char::from(byte));
        }
        Some(s)
    }

    /// An INTEGER with no bounds: a length, then that many octets of two's complement.
    pub fn unconstrained(&mut self) -> Option<i64> {
        let octets = self.length()?;
        if octets == 0 || octets > 8 {
            return None;
        }
        let raw = self.uint(octets * 8)?;
        let shift = 64 - octets * 8;
        Some(((raw << shift) as i64) >> shift)
    }

    /// An OCTET STRING of `min..=max` octets.
    pub fn octets(&mut self, min: i64, max: i64) -> Option<Vec<u8>> {
        let n = self.constrained(min, max)? as usize;
        self.remaining_bytes(n * 8)
    }

    /// An open type's octets: a length, then the encoding it wraps.
    pub fn open(&mut self) -> Option<Vec<u8>> {
        let n = self.length()?;
        self.remaining_bytes(n * 8)
    }

    /// A REAL: a length, then its contents in the X.690 form, binary or decimal.
    pub fn real(&mut self) -> Option<f64> {
        let n = self.length()?;
        let contents = self.remaining_bytes(n * 8)?;
        real_contents(&contents)
    }

    /// The next `nbits` as bytes, a short last byte left aligned.
    pub fn remaining_bytes(&mut self, nbits: usize) -> Option<Vec<u8>> {
        if self.bits.remaining() < nbits {
            return None;
        }
        let mut out = Vec::with_capacity(nbits.div_ceil(8));
        for at in 0..nbits {
            if at % 8 == 0 {
                out.push(0);
            }
            if self.bits.bit()? {
                *out.last_mut()? |= 1 << (7 - at % 8);
            }
        }
        Some(out)
    }
}

const REAL_PLUS_INFINITY: u8 = 0x40;
const REAL_MINUS_INFINITY: u8 = 0x41;
const REAL_NAN: u8 = 0x42;
const REAL_MINUS_ZERO: u8 = 0x43;

fn real_contents(contents: &[u8]) -> Option<f64> {
    let Some((&head, rest)) = contents.split_first() else {
        return Some(0.0);
    };
    if head & 0x80 != 0 {
        return binary_real(head, rest);
    }
    match head {
        REAL_PLUS_INFINITY => Some(f64::INFINITY),
        REAL_MINUS_INFINITY => Some(f64::NEG_INFINITY),
        REAL_NAN => Some(f64::NAN),
        REAL_MINUS_ZERO => Some(-0.0),
        _ if head & 0xC0 == 0 => std::str::from_utf8(rest)
            .ok()?
            .trim()
            .replace(',', ".")
            .parse()
            .ok(),
        _ => None,
    }
}

/// X.690 8.5.7: sign, base 2, 8 or 16, a scale, then the exponent and the mantissa.
fn binary_real(head: u8, rest: &[u8]) -> Option<f64> {
    let sign = if head & 0x40 != 0 { -1.0 } else { 1.0 };
    let base_bits = match (head >> 4) & 0x03 {
        0 => 1,
        1 => 3,
        2 => 4,
        _ => return None,
    };
    let scale = i32::from((head >> 2) & 0x03);
    let (exponent_len, rest) = match head & 0x03 {
        3 => {
            let (&len, rest) = rest.split_first()?;
            (usize::from(len), rest)
        }
        short => (usize::from(short) + 1, rest),
    };
    if exponent_len == 0 || exponent_len > 4 || rest.len() < exponent_len {
        return None;
    }
    let (exponent, mantissa) = rest.split_at(exponent_len);
    let exponent = exponent
        .iter()
        .fold(i64::from(exponent[0] as i8 >> 7), |acc, &b| {
            (acc << 8) | i64::from(b)
        });
    let mantissa = mantissa
        .iter()
        .fold(0.0f64, |acc, &b| acc * 256.0 + f64::from(b));
    let power = i32::try_from(exponent * base_bits)
        .ok()?
        .checked_add(scale)?;
    Some(sign * mantissa * 2f64.powi(power))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_constrained_whole_numbers_and_lengths() {
        let mut per = Per::new(&[0b1011_0000, 0b0000_0011, 0xFF]);
        assert_eq!(per.constrained(0, 15), Some(0b1011));
        assert_eq!(per.uint(4), Some(0));
        assert_eq!(per.length(), Some(3));
        assert_eq!(per.remaining_bytes(5), Some(vec![0b1111_1000]));
        assert_eq!(per.uint(4), None);
    }

    #[test]
    fn a_limit_hides_the_padding_after_it() {
        let mut per = Per::new(&[0xFF]);
        per.limit(3);
        assert_eq!(per.uint(4), None);
        assert_eq!(per.uint(3), Some(0b111));
        assert_eq!(per.bit(), None);
    }
    #[test]
    fn a_restricted_alphabet_sends_indices_when_it_must() {
        const ALPHABET: &[u8] = b"-/0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
        let mut per = Per::new(&[0b0000_0001, 0b1000_0000]);
        assert_eq!(per.restricted(1, 15, ALPHABET).as_deref(), Some("4"));
        let mut per = Per::new(&[0x10, 0x3F]);
        assert_eq!(per.restricted(1, 15, ALPHABET).as_deref(), Some("-z"));
    }

    #[test]
    fn a_wide_alphabet_sends_characters_as_themselves() {
        const ALPHABET: &[u8] =
            b" +-./0123456789=ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz";
        let mut per = Per::new(&[0x01, b'z' << 1]);
        assert_eq!(per.restricted(0, 255, ALPHABET).as_deref(), Some("z"));
        let mut per = Per::new(&[0x01, b'!' << 1]);
        assert_eq!(per.restricted(0, 255, ALPHABET), None);
    }

    #[test]
    fn unconstrained_integers_are_twos_complement() {
        assert_eq!(Per::new(&[0x01, 0x7B]).unconstrained(), Some(123));
        assert_eq!(Per::new(&[0x01, 0xFF]).unconstrained(), Some(-1));
        assert_eq!(Per::new(&[0x02, 0xFF, 0x00]).unconstrained(), Some(-256));
        assert_eq!(Per::new(&[0x00]).unconstrained(), None);
    }

    #[test]
    fn reals_read_binary_decimal_and_special_forms() {
        assert_eq!(Per::new(&[0x03, 0x80, 0xFF, 0x03]).real(), Some(1.5));
        assert_eq!(Per::new(&[0x03, 0xC0, 0x01, 0x05]).real(), Some(-10.0));
        assert_eq!(Per::new(&[0x00]).real(), Some(0.0));
        assert_eq!(Per::new(&[0x01, 0x41]).real(), Some(f64::NEG_INFINITY));
        assert_eq!(
            Per::new(&[0x05, 0x03, b'2', b'.', b'5', b'0']).real(),
            Some(2.5)
        );
    }

    #[test]
    fn octets_and_open_types_carry_their_length() {
        assert_eq!(
            Per::new(&[0x02, 0xAB, 0xCD]).octets(0, 255),
            Some(vec![0xAB, 0xCD])
        );
        assert_eq!(Per::new(&[0x01, 0x60]).open(), Some(vec![0x60]));
    }
}

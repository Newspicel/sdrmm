//! ASN.1 unaligned PER over the packed bit stream both CPDLC flavours share.

use sdrmm_dsp::coding::bits::BitReader;

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
}

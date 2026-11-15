const MAX_MESSAGE_BITS: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct CyclicCode {
    k: u32,
    parity: u32,
    generator: u64,
    correctable: u32,
}

impl CyclicCode {
    pub const GOLAY_20_8: Self = Self {
        k: 8,
        parity: 11,
        generator: 0xC75,
        correctable: 3,
    };

    pub const GOLAY_18_6: Self = Self {
        k: 6,
        parity: 11,
        generator: 0xC75,
        correctable: 3,
    };

    pub const QR_16_7: Self = Self {
        k: 7,
        parity: 8,
        generator: 0x139,
        correctable: 2,
    };

    pub const GOLAY_24_12: Self = Self {
        k: 12,
        parity: 11,
        generator: 0xC75,
        correctable: 3,
    };

    pub const BCH_63_16: Self = Self {
        k: 16,
        parity: 47,
        generator: 0xCD93_0BDD_3B2B,
        correctable: 11,
    };

    #[must_use]
    pub const fn n(&self) -> u32 {
        self.k + self.parity + 1
    }

    fn mask(&self) -> u64 {
        let n = self.n();
        if n >= 64 { u64::MAX } else { (1 << n) - 1 }
    }

    #[must_use]
    pub fn encode(&self, info: u32) -> u64 {
        let info = u64::from(info) & ((1 << self.k) - 1);
        let mut rem = info << self.parity;
        let deg = 63 - self.generator.leading_zeros();
        for shift in (self.parity..self.k + self.parity).rev() {
            if rem >> shift & 1 == 1 {
                rem ^= self.generator << (shift - deg);
            }
        }
        let word = (info << (self.parity + 1)) | (rem << 1);
        word | u64::from(word.count_ones() % 2 == 1)
    }

    #[must_use]
    pub fn decode(&self, word: u64) -> Option<(u32, u32)> {
        let word = word & self.mask();
        let mut rows = [0u64; MAX_MESSAGE_BITS];
        for (i, row) in rows.iter_mut().enumerate().take(self.k as usize) {
            *row = self.encode(1 << i);
        }
        let mut codeword = self.encode(0);
        let mut best = (0u32, (codeword ^ word).count_ones());
        for i in 1..1u32 << self.k {
            codeword ^= rows[i.trailing_zeros() as usize];
            let distance = (codeword ^ word).count_ones();
            if distance < best.1 {
                best = (i ^ (i >> 1), distance);
            }
        }
        (best.1 <= self.correctable).then_some(best)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cyclic_codes_have_their_published_distances() {
        for (code, distance) in [(CyclicCode::GOLAY_20_8, 8), (CyclicCode::QR_16_7, 6)] {
            let min = (1..1u32 << code.k)
                .map(|info| code.encode(info).count_ones())
                .min()
                .unwrap();
            assert_eq!(min, distance);
        }
    }

    #[test]
    fn golay_20_8_matches_the_published_slot_type_words() {
        assert_eq!(CyclicCode::GOLAY_20_8.encode(0x01) >> 1 & 0x7FF, 0x475);
        assert_eq!(CyclicCode::GOLAY_20_8.encode(0x02) >> 1 & 0x7FF, 0x49F);
    }

    #[test]
    fn cyclic_codes_correct_up_to_their_limit_and_refuse_beyond_it() {
        for (code, limit) in [(CyclicCode::GOLAY_20_8, 3), (CyclicCode::QR_16_7, 2)] {
            let info = 0b101;
            let clean = code.encode(info);
            for errors in 0..=limit {
                let mut word = clean;
                for bit in 0..errors {
                    word ^= 1 << (bit * 5 % code.n());
                }
                assert_eq!(code.decode(word), Some((info, errors)), "{errors} errors");
            }
            let mut word = clean;
            for bit in 0..=limit {
                word ^= 1 << (bit * 5 % code.n());
            }
            assert_eq!(code.decode(word), None, "{} errors", limit + 1);
        }
    }
}

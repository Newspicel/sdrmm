mod cyclic;

pub use cyclic::CyclicCode;

#[derive(Clone, Copy, Debug)]
pub struct ParityCode {
    k: usize,
    parity: &'static [u16],
}

impl ParityCode {
    pub const HAMMING_7_4: Self = Self {
        k: 4,
        parity: &[0b0111, 0b1110, 0b1011],
    };

    pub const HAMMING_13_9: Self = Self {
        k: 9,
        parity: &[0b0_0110_1011, 0b0_1101_0111, 0b1_1010_1111, 0b1_0011_0101],
    };

    pub const HAMMING_15_11: Self = Self {
        k: 11,
        parity: &[
            0b001_1010_1111,
            0b011_0101_1110,
            0b110_1011_1100,
            0b100_1101_0111,
        ],
    };

    pub const HAMMING_16_11: Self = Self {
        k: 11,
        parity: &[
            0b001_1010_1111,
            0b011_0101_1110,
            0b110_1011_1100,
            0b100_1101_0111,
            0b111_0110_0101,
        ],
    };

    pub const HAMMING_12_8: Self = Self {
        k: 8,
        parity: &[0b0011_0101, 0b0110_1011, 0b1101_0111, 0b1001_1010],
    };

    pub const HAMMING_10_6: Self = Self {
        k: 6,
        parity: &[0b10_0111, 0b10_1011, 0b01_1101, 0b01_1110],
    };

    pub const HAMMING_17_12: Self = Self {
        k: 12,
        parity: &[
            0b0010_1100_1111,
            0b0101_1001_1111,
            0b1011_0011_1110,
            0b0100_1011_0011,
            0b1001_0110_0111,
        ],
    };

    #[must_use]
    pub const fn k(&self) -> usize {
        self.k
    }

    #[must_use]
    pub const fn n(&self) -> usize {
        self.k + self.parity.len()
    }

    pub fn encode(&self, word: &mut [bool]) {
        assert!(word.len() >= self.n(), "word shorter than the codeword");
        for (j, &mask) in self.parity.iter().enumerate() {
            word[self.k + j] = self.sum(word, mask);
        }
    }

    pub fn decode(&self, word: &mut [bool]) -> Option<u32> {
        assert!(word.len() >= self.n(), "word shorter than the codeword");
        let mut syndrome = 0u32;
        for (j, &mask) in self.parity.iter().enumerate() {
            if self.sum(word, mask) != word[self.k + j] {
                syndrome |= 1 << j;
            }
        }
        if syndrome == 0 {
            return Some(0);
        }
        if syndrome.is_power_of_two() {
            let j = syndrome.trailing_zeros() as usize;
            word[self.k + j] = !word[self.k + j];
            return Some(1);
        }
        for (i, bit) in word.iter_mut().enumerate().take(self.k) {
            if self.column(i) == syndrome {
                *bit = !*bit;
                return Some(1);
            }
        }
        None
    }

    fn sum(&self, word: &[bool], mask: u16) -> bool {
        let mut acc = false;
        for (i, &bit) in word.iter().enumerate().take(self.k) {
            acc ^= mask >> i & 1 == 1 && bit;
        }
        acc
    }

    fn column(&self, i: usize) -> u32 {
        self.parity
            .iter()
            .enumerate()
            .filter(|(_, mask)| *mask >> i & 1 == 1)
            .fold(0, |acc, (j, _)| acc | 1 << j)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn min_distance_parity(code: &ParityCode) -> u32 {
        let mut best = u32::MAX;
        for info in 1..1u32 << code.k() {
            let mut word = vec![false; code.n()];
            for (i, bit) in word.iter_mut().enumerate().take(code.k()) {
                *bit = info >> i & 1 == 1;
            }
            code.encode(&mut word);
            best = best.min(word.iter().filter(|b| **b).count() as u32);
        }
        best
    }

    #[test]
    fn the_hamming_family_has_its_published_distances() {
        assert_eq!(min_distance_parity(&ParityCode::HAMMING_13_9), 3);
        assert_eq!(min_distance_parity(&ParityCode::HAMMING_7_4), 3);
        assert_eq!(min_distance_parity(&ParityCode::HAMMING_15_11), 3);
        assert_eq!(min_distance_parity(&ParityCode::HAMMING_16_11), 4);
        assert_eq!(min_distance_parity(&ParityCode::HAMMING_10_6), 3);
        assert_eq!(min_distance_parity(&ParityCode::HAMMING_12_8), 3);
        assert_eq!(min_distance_parity(&ParityCode::HAMMING_17_12), 3);
    }

    #[test]
    fn parity_codes_repair_any_single_bit() {
        for code in [
            ParityCode::HAMMING_7_4,
            ParityCode::HAMMING_13_9,
            ParityCode::HAMMING_15_11,
            ParityCode::HAMMING_16_11,
            ParityCode::HAMMING_10_6,
            ParityCode::HAMMING_12_8,
            ParityCode::HAMMING_17_12,
        ] {
            let mut clean = vec![false; code.n()];
            for (i, bit) in clean.iter_mut().enumerate().take(code.k()) {
                *bit = i % 3 == 0;
            }
            code.encode(&mut clean);
            for flip in 0..code.n() {
                let mut word = clean.clone();
                word[flip] = !word[flip];
                assert_eq!(code.decode(&mut word), Some(1), "bit {flip}");
                assert_eq!(word, clean, "bit {flip} was repaired to a different word");
            }
        }
    }

    #[test]
    fn hamming_12_8_matches_the_dpmr_generator_matrix() {
        let rows = [
            0b1110, 0b0111, 0b1010, 0b0101, 0b1011, 0b1100, 0b0110, 0b0011,
        ];
        for (data_bit, row) in rows.into_iter().enumerate() {
            let mut word = [false; 12];
            word[data_bit] = true;
            ParityCode::HAMMING_12_8.encode(&mut word);
            let parity = word[8..]
                .iter()
                .fold(0, |acc, &bit| acc << 1 | u8::from(bit));
            assert_eq!(parity, row, "data bit {data_bit}");
        }
    }

    #[test]
    fn the_extended_hamming_detects_double_errors() {
        let code = ParityCode::HAMMING_16_11;
        let mut clean = vec![false; code.n()];
        clean[0] = true;
        clean[4] = true;
        code.encode(&mut clean);
        let mut word = clean.clone();
        word[2] = !word[2];
        word[9] = !word[9];
        assert_eq!(code.decode(&mut word), None);
    }
}

#[derive(Clone, Debug)]
pub struct SyndromeDecoder {
    generator: u128,
    parity_bits: u32,
    corrections: Vec<(u128, u128)>,
}

impl SyndromeDecoder {
    #[must_use]
    pub fn new(generator: u128, n: u32, correctable: u32) -> Self {
        let parity_bits = 127 - generator.leading_zeros();
        assert!(
            parity_bits > 0 && generator & 1 == 1,
            "generator must have degree > 0 and a constant term"
        );
        assert!(
            n > parity_bits && n <= 128,
            "block length must exceed the parity and fit 128 bits"
        );
        let mut decoder = Self {
            generator,
            parity_bits,
            corrections: Vec::new(),
        };
        let singles = (0..n)
            .map(|position| decoder.remainder(1 << position))
            .collect::<Vec<_>>();
        let mut corrections = vec![(0, 0)];
        push_patterns(&singles, 0, correctable, 0, 0, &mut corrections);
        corrections.sort_unstable_by_key(|&(syndrome, pattern)| (syndrome, pattern.count_ones()));
        corrections.dedup_by_key(|&mut (syndrome, _)| syndrome);
        decoder.corrections = corrections;
        decoder
    }

    #[must_use]
    pub const fn parity_bits(&self) -> u32 {
        self.parity_bits
    }

    #[must_use]
    pub fn remainder(&self, word: u128) -> u128 {
        let mut rem = word;
        for bit in (self.parity_bits..128).rev() {
            if rem >> bit & 1 == 1 {
                rem ^= self.generator << (bit - self.parity_bits);
            }
        }
        rem
    }

    #[must_use]
    pub fn encode(&self, info: u128) -> u128 {
        let shifted = info << self.parity_bits;
        shifted | self.remainder(shifted)
    }

    #[must_use]
    pub fn decode(&self, word: u128) -> Option<(u128, u32)> {
        let syndrome = self.remainder(word);
        let at = self
            .corrections
            .binary_search_by_key(&syndrome, |&(known, _)| known)
            .ok()?;
        let pattern = self.corrections[at].1;
        Some((word ^ pattern, pattern.count_ones()))
    }
}

fn push_patterns(
    singles: &[u128],
    start: usize,
    remaining: u32,
    syndrome: u128,
    pattern: u128,
    out: &mut Vec<(u128, u128)>,
) {
    if remaining == 0 {
        return;
    }
    for (position, single) in singles.iter().enumerate().skip(start) {
        let syndrome = syndrome ^ single;
        let pattern = pattern | 1 << position;
        out.push((syndrome, pattern));
        push_patterns(singles, position + 1, remaining - 1, syndrome, pattern, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BCH_63_45_RECIPROCAL: u128 = 0b111_1001_1010_0000_1111;
    const BCH_63_30_RECIPROCAL: u128 = 0b11_1001_1011_0101_1100_0010_1100_1111_1011;
    const BCH_127_106: u128 = 0b10_0110_1101_1001_1110_0011;

    fn spread(seed: u64, count: u32, n: u64) -> u128 {
        let mut state = seed | 1;
        let mut pattern = 0u128;
        while pattern.count_ones() < count {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            pattern |= 1 << (state % n);
        }
        pattern
    }

    #[test]
    fn a_codeword_divides_by_the_generator() {
        let code = SyndromeDecoder::new(BCH_63_45_RECIPROCAL, 63, 2);
        let word = code.encode(0x0123_4567_89AB);
        assert_eq!(code.remainder(word), 0);
        assert_eq!(code.decode(word), Some((word, 0)));
    }

    #[test]
    fn every_pattern_up_to_the_designed_distance_has_its_own_syndrome() {
        for (generator, t) in [(BCH_63_45_RECIPROCAL, 3), (BCH_63_30_RECIPROCAL, 3)] {
            let code = SyndromeDecoder::new(generator, 63, t);
            let patterns = (0..=t).map(|w| binomial(63, w)).sum::<u64>();
            assert_eq!(code.corrections.len() as u64, patterns, "{generator:#x}");
        }
    }

    #[test]
    fn corrects_up_to_its_limit_and_refuses_one_more() {
        let code = SyndromeDecoder::new(BCH_63_30_RECIPROCAL, 63, 3);
        let clean = code.encode(0x2AAA_5555);
        for seed in 1..200 {
            for errors in 0..=3 {
                let word = clean ^ spread(seed * 7 + u64::from(errors), errors, 63);
                assert_eq!(code.decode(word), Some((clean, errors)));
            }
        }
        let strict = SyndromeDecoder::new(BCH_63_45_RECIPROCAL, 63, 1);
        let clean = strict.encode(0x00F0_F0F0_F0F0);
        assert_eq!(strict.decode(clean ^ 0b101), None);
    }

    #[test]
    fn a_shortened_code_wider_than_64_bits_corrects_its_full_length() {
        let code = SyndromeDecoder::new(BCH_127_106, 82, 3);
        let clean = code.encode(0x0ABC_DEF0_1234_5678);
        assert_eq!(code.remainder(clean), 0);
        for seed in 1..100 {
            let word = clean ^ spread(seed, 3, 82);
            assert_eq!(code.decode(word), Some((clean, 3)));
        }
    }

    fn binomial(n: u64, k: u32) -> u64 {
        (0..u64::from(k)).fold(1, |acc, i| acc * (n - i) / (i + 1))
    }
}

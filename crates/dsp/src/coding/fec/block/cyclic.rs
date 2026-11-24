use crate::coding::fec::{
    conv::Soft,
    soft_ml::{confident, decode, span},
};

const MAX_MESSAGE_BITS: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct CyclicCode {
    k: u32,
    parity: u32,
    generator: u64,
    distance: u32,
}

impl CyclicCode {
    pub const GOLAY_20_8: Self = Self {
        k: 8,
        parity: 11,
        generator: 0xC75,
        distance: 8,
    };

    pub const GOLAY_18_6: Self = Self {
        k: 6,
        parity: 11,
        generator: 0xC75,
        distance: 8,
    };

    pub const QR_16_7: Self = Self {
        k: 7,
        parity: 8,
        generator: 0x139,
        distance: 6,
    };

    pub const GOLAY_24_12: Self = Self {
        k: 12,
        parity: 11,
        generator: 0xC75,
        distance: 8,
    };

    pub const BCH_63_16: Self = Self {
        k: 16,
        parity: 47,
        generator: 0xCD93_0BDD_3B2B,
        distance: 24,
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
        let deg = self.generator.ilog2();
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
        let rows = self.rows();
        let mut best = (0u32, word.count_ones());
        for (info, codeword) in span(&rows[..self.k as usize]) {
            let distance = (codeword ^ word).count_ones();
            if distance < best.1 {
                best = (info, distance);
            }
        }
        (2 * best.1 < self.distance).then_some(best)
    }

    /// Soft maximum likelihood over every codeword: `soft` holds one value a bit, first sent
    /// first, positive for a one. Returns the message and the decisions it overturns, or `None`
    /// when `soft` is not `n` long or the best codeword does not stand. Hard input decodes as
    /// [`Self::decode`] does.
    #[must_use]
    pub fn decode_soft(&self, soft: &[Soft]) -> Option<(u32, u32)> {
        if soft.len() != self.n() as usize {
            return None;
        }
        let (values, len) = confident(soft);
        let rows = self.rows();
        decode(&rows[..self.k as usize], &values[..len], self.distance)
            .map(|(info, _, overturned)| (info, overturned))
    }

    fn rows(&self) -> [u64; MAX_MESSAGE_BITS] {
        std::array::from_fn(|i| {
            if i < self.k as usize {
                self.encode(1 << i)
            } else {
                0
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding::fec::{
        conv::{CONFIDENT, soft},
        soft_ml::SoftWord,
    };

    const CODES: [CyclicCode; 5] = [
        CyclicCode::GOLAY_20_8,
        CyclicCode::GOLAY_18_6,
        CyclicCode::QR_16_7,
        CyclicCode::GOLAY_24_12,
        CyclicCode::BCH_63_16,
    ];

    fn soften(code: &CyclicCode, word: u64) -> Vec<Soft> {
        let n = code.n();
        (0..n).map(|i| soft(word >> (n - 1 - i) & 1 == 1)).collect()
    }

    fn spread(code: &CyclicCode, errors: u32) -> u64 {
        (0..errors).fold(0, |mask, e| mask | 1 << (e * 7 % code.n()))
    }

    #[test]
    fn the_cyclic_codes_have_their_published_distances() {
        for code in CODES {
            let min = (1..1u32 << code.k)
                .map(|info| code.encode(info).count_ones())
                .min()
                .unwrap();
            assert_eq!(min, code.distance, "n {}", code.n());
        }
    }

    #[test]
    fn soft_decoding_of_hard_input_matches_the_hard_decoder() {
        for code in CODES {
            let t = (code.distance - 1) / 2;
            for info in [0, 0b101, (1 << code.k) - 1] {
                let clean = code.encode(info);
                for errors in 0..=t + 2 {
                    let word = clean ^ spread(&code, errors);
                    assert_eq!(
                        code.decode_soft(&soften(&code, word)),
                        code.decode(word),
                        "n {} info {info} errors {errors}",
                        code.n()
                    );
                }
            }
        }
    }

    #[test]
    fn weak_errors_beyond_the_hard_radius_are_corrected() {
        for code in CODES {
            let info = 0b1_0110;
            let errors = (code.distance - 1) / 2 + 1;
            let flips = spread(&code, errors);
            let word = code.encode(info) ^ flips;
            assert_eq!(code.decode(word), None, "n {}", code.n());
            let mut received = soften(&code, word);
            for (i, value) in received.iter_mut().enumerate() {
                if flips >> (code.n() as usize - 1 - i) & 1 == 1 {
                    *value /= 4;
                }
            }
            let expected = Some((info & ((1 << code.k) - 1), errors));
            assert_eq!(code.decode_soft(&received), expected, "n {}", code.n());
        }
    }

    #[test]
    fn erasures_fill_in_up_to_the_distance() {
        let code = CyclicCode::GOLAY_20_8;
        let clean = code.encode(0xA5);
        let mut received = soften(&code, clean ^ 0b11);
        received[..3].fill(0);
        assert_eq!(code.decode_soft(&received), Some((0xA5, 2)));
        received[3] = 0;
        assert_eq!(code.decode_soft(&received), None);
        assert_eq!(code.decode_soft(&[0; 20]), None);
        assert_eq!(code.decode_soft(&soften(&code, clean)[..19]), None);
    }

    #[test]
    fn a_uniformly_weak_clean_word_still_decodes() {
        for code in CODES {
            let clean = code.encode(0x2D);
            let weak: Vec<Soft> = soften(&code, clean).iter().map(|v| v / 8).collect();
            assert!(code.decode_soft(&weak).is_some(), "n {}", code.n());
        }
    }

    /// The acceptance before it scaled reliability by the word's mean: Forney's test against the
    /// surest bit, or the hard rule.
    fn stood_against_the_peak(code: &CyclicCode, received: &[Soft]) -> bool {
        let (values, len) = confident(received);
        let word = SoftWord::new(&values[..len]);
        let rows = code.rows();
        let (_, codeword) = word.best(span(&rows[..code.k as usize]));
        let peak = values
            .iter()
            .fold(0.0f32, |peak, value| peak.max(value.abs()));
        let erasures = values[..len].iter().filter(|&&value| value == 0.0).count() as u32;
        let room = (code.n() - code.distance) as f32;
        2 * word.overturned(codeword) + erasures < code.distance
            || word.correlation(codeword) > room * peak
    }

    struct Noise(u64);

    impl Noise {
        fn level(&mut self) -> Soft {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % (2 * CONFIDENT as u64 + 1)) as Soft - CONFIDENT
        }
    }

    #[test]
    fn random_words_pass_no_more_often_than_against_the_peak() {
        for (code, words) in [
            (CyclicCode::GOLAY_24_12, 4000),
            (CyclicCode::BCH_63_16, 400),
        ] {
            let mut noise = Noise(0x9E37_79B9_7F4A_7C15);
            let (mut now, mut before) = (0, 0);
            for _ in 0..words {
                let received: Vec<Soft> = (0..code.n()).map(|_| noise.level()).collect();
                now += usize::from(code.decode_soft(&received).is_some());
                before += usize::from(stood_against_the_peak(&code, &received));
            }
            assert!(now <= before, "n {}: {now} now, {before} before", code.n());
        }
    }

    #[test]
    fn weak_errors_among_inner_and_outer_levels_are_corrected() {
        for code in [CyclicCode::GOLAY_24_12, CyclicCode::BCH_63_16] {
            let info = 0b1_0110_1101;
            let errors = (code.distance - 1) / 2 + 1;
            let flips = spread(&code, errors);
            let clean = soften(&code, code.encode(info));
            let received: Vec<Soft> = clean
                .iter()
                .enumerate()
                .map(|(i, &value)| {
                    let place = code.n() as usize - 1 - i;
                    if flips >> place & 1 == 1 {
                        -value / 8
                    } else if i % 2 == 1 {
                        value / 2
                    } else {
                        value
                    }
                })
                .collect();
            assert!(!stood_against_the_peak(&code, &received), "n {}", code.n());
            assert_eq!(
                code.decode_soft(&received),
                Some((info, errors)),
                "n {}",
                code.n()
            );
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

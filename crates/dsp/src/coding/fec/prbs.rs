#[derive(Clone, Copy, Debug)]
pub struct PrbsSpec {
    pub stages: u32,
    pub taps: u32,
    pub seed: u32,
}

#[derive(Clone, Debug)]
pub struct Prbs {
    spec: PrbsSpec,
    register: u32,
    mask: u32,
}

impl Prbs {
    #[must_use]
    pub const fn new(spec: PrbsSpec) -> Self {
        assert!(
            spec.stages >= 1 && spec.stages <= u32::BITS,
            "the register does not fit a u32"
        );
        Self {
            spec,
            register: spec.seed,
            mask: u32::MAX >> (u32::BITS - spec.stages),
        }
    }

    /// The register itself, for generators that read it whole as a byte or word per step.
    #[must_use]
    pub const fn register(&self) -> u32 {
        self.register
    }

    pub const fn reset(&mut self) {
        self.register = self.spec.seed;
    }

    pub const fn next_bit(&mut self) -> bool {
        let feedback = (self.register & self.spec.taps).count_ones() % 2;
        self.register = (self.register << 1 | feedback) & self.mask;
        feedback == 1
    }

    /// Steps like [`Self::next_bit`] but returns the bit leaving the register: the same
    /// sequence `stages` steps earlier, as a register read from its far end sends it.
    pub const fn next_tail_bit(&mut self) -> bool {
        let tail = self.register >> (self.spec.stages - 1) & 1 == 1;
        self.next_bit();
        tail
    }

    pub fn apply_bits(&mut self, bits: &mut [bool]) {
        for bit in bits {
            *bit ^= self.next_bit();
        }
    }

    pub fn apply_bytes(&mut self, data: &mut [u8]) {
        for byte in data {
            let mut scrambled = 0u8;
            for index in (0..8).rev() {
                scrambled |= u8::from(self.next_bit()) << index;
            }
            *byte ^= scrambled;
        }
    }

    pub fn skip_bytes(&mut self, count: usize) {
        for _ in 0..count * 8 {
            self.next_bit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const X9_X5: PrbsSpec = PrbsSpec {
        stages: 9,
        taps: 0x110,
        seed: 0x1FF,
    };

    const X15_X14: PrbsSpec = PrbsSpec {
        stages: 15,
        taps: 0x6000,
        seed: 0x00A9,
    };

    fn sequence(spec: PrbsSpec, len: usize) -> Vec<bool> {
        let mut prbs = Prbs::new(spec);
        (0..len).map(|_| prbs.next_bit()).collect()
    }

    fn period(spec: PrbsSpec) -> usize {
        let mut prbs = Prbs::new(spec);
        let mut steps = 0usize;
        loop {
            prbs.next_bit();
            steps += 1;
            if prbs.register == spec.seed {
                return steps;
            }
        }
    }

    #[test]
    fn the_register_reads_whole_before_each_step() {
        let mut prbs = Prbs::new(PrbsSpec {
            stages: 8,
            taps: 0xb8,
            seed: 0xff,
        });
        let states: Vec<u32> = (0..6)
            .map(|_| {
                let state = prbs.register();
                prbs.next_bit();
                state
            })
            .collect();
        assert_eq!(states, [0xff, 0xfe, 0xfc, 0xf8, 0xf0, 0xe1]);
    }

    #[test]
    fn both_generators_are_maximal_length() {
        assert_eq!(period(X9_X5), 511);
        assert_eq!(period(X15_X14), 32_767);
    }

    #[test]
    fn the_x15_sequence_starts_with_the_bits_the_seed_implies() {
        let bits = sequence(X15_X14, 8);
        assert_eq!(bits, [false, false, false, false, false, false, true, true]);
    }

    #[test]
    fn the_x9_sequence_starts_with_the_bits_the_all_ones_seed_implies() {
        let bits = sequence(X9_X5, 6);
        assert_eq!(bits, [false, false, false, false, false, true]);
    }

    #[test]
    fn a_register_filling_the_word_keeps_its_whole_mask() {
        let spec = PrbsSpec {
            stages: 32,
            taps: 0x8020_0003,
            seed: 0x1,
        };
        let mut prbs = Prbs::new(spec);
        let bits: Vec<bool> = (0..64).map(|_| prbs.next_bit()).collect();
        assert!(bits.iter().any(|&bit| bit), "a full mask produced no ones");
    }

    #[test]
    #[should_panic(expected = "does not fit a u32")]
    fn a_register_wider_than_the_word_is_refused() {
        let _ = Prbs::new(PrbsSpec {
            stages: 33,
            taps: 0x3,
            seed: 0x1,
        });
    }

    #[test]
    fn dispersal_is_its_own_inverse() {
        let original: Vec<u8> = (0..188u16).map(|value| (value * 7) as u8).collect();
        let mut data = original.clone();
        Prbs::new(X9_X5).apply_bytes(&mut data);
        assert_ne!(data, original);
        Prbs::new(X9_X5).apply_bytes(&mut data);
        assert_eq!(data, original);
    }

    #[test]
    fn skipping_matches_applying_to_a_scratch_buffer() {
        let mut skipped = Prbs::new(X15_X14);
        skipped.skip_bytes(4);
        let mut applied = Prbs::new(X15_X14);
        applied.apply_bytes(&mut [0u8; 4]);
        assert_eq!(skipped.register, applied.register);
    }

    #[test]
    fn the_tail_runs_the_sequence_stages_steps_behind() {
        let mut lead = Prbs::new(X15_X14);
        let mut tail = Prbs::new(X15_X14);
        let leading: Vec<bool> = (0..200).map(|_| lead.next_bit()).collect();
        let trailing: Vec<bool> = (0..215).map(|_| tail.next_tail_bit()).collect();
        assert_eq!(trailing[15..], leading[..]);
        let seed: Vec<bool> = (0..15)
            .rev()
            .map(|bit| X15_X14.seed >> bit & 1 == 1)
            .collect();
        assert_eq!(trailing[..15], seed[..]);
    }

    #[test]
    fn a_sequence_can_be_built_at_compile_time() {
        const FIRST: [bool; 8] = {
            let mut prbs = Prbs::new(X15_X14);
            let mut out = [false; 8];
            let mut index = 0;
            while index < out.len() {
                out[index] = prbs.next_bit();
                index += 1;
            }
            out
        };
        assert_eq!(
            FIRST,
            [false, false, false, false, false, false, true, true]
        );
    }

    #[test]
    fn the_x9_sequence_is_balanced_over_one_period() {
        let ones = sequence(X9_X5, 511).into_iter().filter(|&bit| bit).count();
        assert_eq!(ones, 256);
    }
}

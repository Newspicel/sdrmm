use super::remainder::remainder;

const AMBIGUOUS: u32 = u32::MAX;

/// A shortened cyclic code of up to 32 bits whose check field carries an offset word, repairing
/// any burst up to `max_burst` bits long through a syndrome table. `S` is `1 << check_bits`.
#[derive(Clone, Copy, Debug)]
pub struct BurstCode<const S: usize> {
    generator: u32,
    block_bits: u32,
    check_bits: u32,
    table: [u32; S],
}

impl<const S: usize> BurstCode<S> {
    #[must_use]
    pub const fn new(generator: u32, block_bits: u32, check_bits: u32, max_burst: u32) -> Self {
        assert!(S == 1 << check_bits, "one table entry per syndrome");
        assert!(
            block_bits <= 32 && check_bits < block_bits,
            "block fits 32 bits"
        );
        let mut table = [0u32; S];
        let mut shift = 0;
        while shift < block_bits {
            let mut pattern = 1u32;
            while pattern < 1 << max_burst {
                let vector = pattern << shift;
                if vector >> block_bits == 0 {
                    let at = remainder(vector, generator, block_bits, check_bits) as usize;
                    if table[at] == 0 {
                        table[at] = vector;
                    } else if table[at] != vector {
                        table[at] = AMBIGUOUS;
                    }
                }
                pattern += 1;
            }
            shift += 1;
        }
        Self {
            generator,
            block_bits,
            check_bits,
            table,
        }
    }

    #[must_use]
    pub const fn syndrome(&self, block: u32) -> u32 {
        remainder(block, self.generator, self.block_bits, self.check_bits)
    }

    /// The data of a block that checks clean against `offset`.
    #[must_use]
    pub fn check(&self, block: u32, offset: u32) -> Option<u32> {
        (self.syndrome(block) == offset).then_some(block >> self.check_bits)
    }

    /// The data after repairing one burst, with the number of bits flipped.
    #[must_use]
    pub fn correct(&self, block: u32, offset: u32) -> Option<(u32, u32)> {
        let syndrome = self.syndrome(block) ^ offset;
        if syndrome == 0 {
            return Some((block >> self.check_bits, 0));
        }
        match self.table.get(syndrome as usize).copied() {
            Some(0 | AMBIGUOUS) | None => None,
            Some(vector) => Some(((block ^ vector) >> self.check_bits, vector.count_ones())),
        }
    }

    #[must_use]
    pub fn encode(&self, data: u32, offset: u32) -> u32 {
        let payload = data << self.check_bits;
        payload | (self.syndrome(payload) ^ offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK_BITS: u32 = 26;
    const MAX_BURST: u32 = 5;
    const CODE: BurstCode<1024> = BurstCode::new(0x5B9, BLOCK_BITS, 10, MAX_BURST);
    const OFFSETS: [u32; 5] = [0x0FC, 0x198, 0x168, 0x350, 0x1B4];

    #[test]
    fn every_offset_round_trips() {
        for offset in OFFSETS {
            for data in [0x0000, 0xFFFF, 0x1234, 0xACDC] {
                let block = CODE.encode(data, offset);
                assert_eq!(block >> BLOCK_BITS, 0, "block must fit 26 bits");
                assert_eq!(CODE.syndrome(block), offset);
                assert_eq!(CODE.check(block, offset), Some(data));
            }
        }
    }

    #[test]
    fn any_single_bit_flip_fails_the_check() {
        let block = CODE.encode(0x3A5C, OFFSETS[1]);
        for bit in 0..BLOCK_BITS {
            assert_eq!(
                CODE.check(block ^ (1 << bit), OFFSETS[1]),
                None,
                "bit {bit}"
            );
        }
    }

    #[test]
    fn every_burst_up_to_the_limit_is_repaired() {
        for offset in OFFSETS {
            for data in [0x0000, 0xFFFF, 0x3A5C, 0xACDC] {
                let block = CODE.encode(data, offset);
                assert_eq!(CODE.correct(block, offset), Some((data, 0)));
                for shift in 0..BLOCK_BITS {
                    for pattern in 1u32..1 << MAX_BURST {
                        let vector = pattern << shift;
                        if vector >> BLOCK_BITS != 0 {
                            continue;
                        }
                        assert_eq!(
                            CODE.correct(block ^ vector, offset),
                            Some((data, vector.count_ones())),
                            "{offset:#x} burst {pattern:#b} at {shift}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn no_syndrome_points_at_two_patterns() {
        assert!(!CODE.table.contains(&AMBIGUOUS));
        let reachable = CODE.table.iter().filter(|&&v| v != 0).count();
        assert_eq!(reachable, 367, "one syndrome per correctable burst");
    }

    #[test]
    fn a_longer_burst_can_be_mistaken_for_a_shorter_one() {
        let longer: Vec<u32> = (0..BLOCK_BITS)
            .flat_map(|shift| (1u32 << 5..1 << 6).map(move |pattern| pattern << shift))
            .filter(|&vector| vector >> BLOCK_BITS == 0 && vector & 1 != 0)
            .collect();
        assert!(
            longer
                .iter()
                .any(|&vector| CODE.table[CODE.syndrome(vector) as usize] != vector)
        );
    }

    #[test]
    fn a_block_checked_against_the_wrong_offset_fails() {
        for offset in OFFSETS {
            let block = CODE.encode(0x5A5A, offset);
            for other in OFFSETS.into_iter().filter(|&other| other != offset) {
                assert_eq!(CODE.check(block, other), None, "{offset:#x} vs {other:#x}");
            }
        }
    }
}

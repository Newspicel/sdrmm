use super::remainder::remainder;

const RDS_GEN: u32 = 0x5B9;
const RDS_BLOCK_BITS: u32 = 26;
const RDS_CHECK_BITS: u32 = 10;
const RDS_MAX_BURST: u32 = 5;
const RDS_SYNDROMES: usize = 1 << RDS_CHECK_BITS;
const RDS_AMBIGUOUS: u32 = u32::MAX;

const RDS_BURST_TABLE: [u32; RDS_SYNDROMES] = {
    let mut table = [0u32; RDS_SYNDROMES];
    let mut shift = 0;
    while shift < RDS_BLOCK_BITS {
        let mut pattern = 1u32;
        while pattern < 1 << RDS_MAX_BURST {
            let vector = pattern << shift;
            if vector >> RDS_BLOCK_BITS == 0 {
                let at = rds_syndrome(vector) as usize;
                if table[at] == 0 {
                    table[at] = vector;
                } else if table[at] != vector {
                    table[at] = RDS_AMBIGUOUS;
                }
            }
            pattern += 1;
        }
        shift += 1;
    }
    table
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RdsOffset {
    A,
    B,
    C,
    CPrime,
    D,
}

impl RdsOffset {
    #[must_use]
    pub const fn word(self) -> u16 {
        match self {
            Self::A => 0x0FC,
            Self::B => 0x198,
            Self::C => 0x168,
            Self::CPrime => 0x350,
            Self::D => 0x1B4,
        }
    }
}

#[must_use]
pub const fn rds_syndrome(block: u32) -> u16 {
    remainder(block, RDS_GEN, RDS_BLOCK_BITS, RDS_CHECK_BITS) as u16
}

#[must_use]
pub fn rds_check_block(block: u32, offset: RdsOffset) -> Option<u16> {
    (rds_syndrome(block) == offset.word()).then_some((block >> RDS_CHECK_BITS) as u16)
}

#[must_use]
pub fn rds_correct_block(block: u32, offset: RdsOffset) -> Option<(u16, u32)> {
    let syndrome = rds_syndrome(block) ^ offset.word();
    if syndrome == 0 {
        return Some(((block >> RDS_CHECK_BITS) as u16, 0));
    }
    match RDS_BURST_TABLE.get(usize::from(syndrome)).copied() {
        Some(0) | Some(RDS_AMBIGUOUS) | None => None,
        Some(vector) => Some((
            ((block ^ vector) >> RDS_CHECK_BITS) as u16,
            vector.count_ones(),
        )),
    }
}

#[must_use]
pub fn rds_encode_block(data: u16, offset: RdsOffset) -> u32 {
    let payload = u32::from(data) << RDS_CHECK_BITS;
    payload | u32::from(rds_syndrome(payload) ^ offset.word())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RDS_OFFSETS: [RdsOffset; 5] = [
        RdsOffset::A,
        RdsOffset::B,
        RdsOffset::C,
        RdsOffset::CPrime,
        RdsOffset::D,
    ];

    #[test]
    fn rds_round_trips_every_offset() {
        for offset in RDS_OFFSETS {
            for data in [0x0000, 0xFFFF, 0x1234, 0xACDC] {
                let block = rds_encode_block(data, offset);
                assert_eq!(block >> RDS_BLOCK_BITS, 0, "block must fit 26 bits");
                assert_eq!(rds_syndrome(block), offset.word());
                assert_eq!(rds_check_block(block, offset), Some(data));
            }
        }
    }

    #[test]
    fn rds_rejects_any_single_bit_flip() {
        let block = rds_encode_block(0x3A5C, RdsOffset::B);
        for bit in 0..RDS_BLOCK_BITS {
            let corrupt = block ^ (1 << bit);
            assert_eq!(
                rds_check_block(corrupt, RdsOffset::B),
                None,
                "bit {bit} slipped past the block check"
            );
        }
    }

    #[test]
    fn rds_repairs_every_burst_of_span_five_or_less() {
        for offset in RDS_OFFSETS {
            for data in [0x0000, 0xFFFF, 0x3A5C, 0xACDC] {
                let block = rds_encode_block(data, offset);
                assert_eq!(rds_correct_block(block, offset), Some((data, 0)));
                for shift in 0..RDS_BLOCK_BITS {
                    for pattern in 1u32..1 << RDS_MAX_BURST {
                        let vector = pattern << shift;
                        if vector >> RDS_BLOCK_BITS != 0 {
                            continue;
                        }
                        assert_eq!(
                            rds_correct_block(block ^ vector, offset),
                            Some((data, vector.count_ones())),
                            "{offset:?} burst {pattern:#b} at {shift}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn rds_correction_leaves_no_syndrome_pointing_at_two_error_patterns() {
        assert!(
            !RDS_BURST_TABLE.contains(&RDS_AMBIGUOUS),
            "the burst table cannot resolve every pattern it holds"
        );
        let reachable = RDS_BURST_TABLE.iter().filter(|&&v| v != 0).count();
        assert_eq!(reachable, 367, "one syndrome per correctable burst");
    }

    #[test]
    fn rds_correction_stops_where_the_code_stops_being_unambiguous() {
        let span_six: Vec<u32> = (0..RDS_BLOCK_BITS)
            .flat_map(|shift| (1u32 << 5..1 << 6).map(move |pattern| pattern << shift))
            .filter(|&vector| vector >> RDS_BLOCK_BITS == 0 && vector & 1 != 0)
            .collect();
        assert!(
            span_six
                .iter()
                .any(|&vector| RDS_BURST_TABLE[usize::from(rds_syndrome(vector))] != vector),
            "a span-six burst would still be mistaken for a shorter one"
        );
    }

    #[test]
    fn rds_rejects_a_block_checked_against_the_wrong_offset() {
        for offset in RDS_OFFSETS {
            let block = rds_encode_block(0x5A5A, offset);
            for other in RDS_OFFSETS {
                if other == offset {
                    continue;
                }
                assert_eq!(
                    rds_check_block(block, other),
                    None,
                    "{offset:?} vs {other:?}"
                );
            }
        }
    }
}

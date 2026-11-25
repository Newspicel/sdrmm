/// How far a run of soft values (positive is one) sits from a sync word, in bit errors, read
/// upright and inverted: the magnitude on the wrong side over the mean magnitude, and half a
/// bit for each erased (zero) value. With values all of one size it is the Hamming distance,
/// so a threshold in bit errors keeps its meaning; with real soft values a weak wrong bit costs
/// less than a confident one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SyncDistance {
    pub upright: f32,
    pub inverted: f32,
}

impl SyncDistance {
    /// The nearer reading, and whether it is the inverted one.
    #[must_use]
    pub fn best(self) -> (f32, bool) {
        if self.upright <= self.inverted {
            (self.upright, false)
        } else {
            (self.inverted, true)
        }
    }

    /// Both readings of hard bits `window`: the Hamming distances.
    #[must_use]
    pub fn hard(window: u64, word: u64, mask: u64) -> Self {
        Self {
            upright: ((window ^ word) & mask).count_ones() as f32,
            inverted: ((!window ^ word) & mask).count_ones() as f32,
        }
    }
}

/// `soft` holds one value per bit of `word`, MSB first; only bits set in `mask` count.
#[must_use]
pub fn sync_distance<T: Copy + Into<f32>>(soft: &[T], word: u64, mask: u64) -> SyncDistance {
    let last = soft.len().saturating_sub(1);
    let (mut wrong, mut total, mut heard, mut erased) = (0.0f32, 0.0f32, 0u32, 0u32);
    for (index, &value) in soft.iter().enumerate() {
        let shift = (last - index) as u32;
        if mask.checked_shr(shift).is_none_or(|bits| bits & 1 == 0) {
            continue;
        }
        let value: f32 = value.into();
        if value == 0.0 {
            erased += 1;
            continue;
        }
        heard += 1;
        total += value.abs();
        if (value > 0.0) != (word >> shift & 1 == 1) {
            wrong += value.abs();
        }
    }
    scaled(wrong, total, heard as f32, erased as f32)
}

fn scaled(wrong: f32, total: f32, heard: f32, erased: f32) -> SyncDistance {
    let unsure = erased / 2.0;
    let scale = if total > 0.0 { heard / total } else { 0.0 };
    SyncDistance {
        upright: wrong * scale + unsure,
        inverted: (total - wrong) * scale + unsure,
    }
}

/// Partial sums kept apart, so judging a window vectorizes.
const LANES: usize = 8;

/// A sync word laid out against the newest 64 soft values, so judging a window takes no
/// shifts or branches.
#[derive(Clone, Debug)]
pub struct SyncPattern {
    signs: [f32; 64],
    counts: [f32; 64],
    counted: f32,
}

impl SyncPattern {
    /// Only bits set in `mask` count.
    #[must_use]
    pub fn new(word: u64, mask: u64) -> Self {
        let counts: [f32; 64] = std::array::from_fn(|index| bit(mask, index) as f32);
        Self {
            signs: std::array::from_fn(|index| {
                counts[index] * (2.0 * bit(word, index) as f32 - 1.0)
            }),
            counts,
            counted: mask.count_ones() as f32,
        }
    }

    /// As [`sync_distance`] over `soft`, the newest 64 values, the newest last.
    #[must_use]
    pub fn distance(&self, soft: &[f32; 64]) -> SyncDistance {
        let mut magnitude = [0.0f32; LANES];
        let mut agreement = [0.0f32; LANES];
        let mut erased = [0.0f32; LANES];
        let (values, _) = soft.as_chunks::<LANES>();
        let (signs, _) = self.signs.as_chunks::<LANES>();
        let (counts, _) = self.counts.as_chunks::<LANES>();
        for ((values, signs), counts) in values.iter().zip(signs).zip(counts) {
            for lane in 0..LANES {
                let value = values[lane];
                magnitude[lane] += counts[lane] * value.abs();
                agreement[lane] += signs[lane] * value;
                erased[lane] += if value == 0.0 { counts[lane] } else { 0.0 };
            }
        }
        let total: f32 = magnitude.iter().sum();
        let erased: f32 = erased.iter().sum();
        let wrong = (total - agreement.iter().sum::<f32>()) / 2.0;
        scaled(wrong, total, self.counted - erased, erased)
    }
}

/// The bit of `word` that lines up with `index` of 64 values, the last one bit 0.
const fn bit(word: u64, index: usize) -> u64 {
    word >> (63 - index) & 1
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORD: u64 = 0b1011_0010;

    fn levels(word: u64, size: f32) -> Vec<f32> {
        (0..8)
            .rev()
            .map(|shift| if word >> shift & 1 == 1 { size } else { -size })
            .collect()
    }

    #[test]
    fn equal_magnitudes_count_bit_errors_both_ways() {
        let mut soft = levels(WORD, 3.0);
        soft[1] = -soft[1];
        soft[6] = -soft[6];
        let distance = sync_distance(&soft, WORD, 0xFF);
        assert_eq!(
            distance,
            SyncDistance {
                upright: 2.0,
                inverted: 6.0
            }
        );
        assert_eq!(distance, SyncDistance::hard(WORD ^ 0b0100_0010, WORD, 0xFF));
        let inverted: Vec<i16> = levels(!WORD, 40.0).iter().map(|&v| v as i16).collect();
        assert_eq!(sync_distance(&inverted, WORD, 0xFF).best(), (0.0, true));
    }

    #[test]
    fn a_weak_wrong_bit_costs_less_than_a_confident_one() {
        let mut soft = levels(WORD, 1.0);
        soft[2] = -0.1;
        soft[4] = 0.1;
        let weak = sync_distance(&soft, WORD, 0xFF).upright;
        assert!(weak < 0.3, "{weak}");
        soft[2] = -1.0;
        soft[4] = 1.0;
        assert_eq!(sync_distance(&soft, WORD, 0xFF).upright, 2.0);
    }

    #[test]
    fn a_pattern_judges_the_newest_values_as_the_plain_search_does() {
        let mut soft: [f32; 64] = std::array::from_fn(|index| (index as f32 * 0.37).sin() * 3.0);
        soft[60] = 0.0;
        for (word, mask) in [(WORD, 0xFFu64), (0x1ACF_FC1D, 0xFFFF_FFFF), (WORD, 0x7F)] {
            let bits = 64 - mask.leading_zeros() as usize;
            let plain = sync_distance(&soft[64 - bits..], word, mask);
            let laid = SyncPattern::new(word, mask).distance(&soft);
            assert!(
                (plain.upright - laid.upright).abs() < 1e-4,
                "{plain:?} {laid:?}"
            );
            assert!(
                (plain.inverted - laid.inverted).abs() < 1e-4,
                "{plain:?} {laid:?}"
            );
        }
    }

    #[test]
    fn masked_bits_do_not_count_and_erased_ones_count_half() {
        let mut soft = levels(WORD, 1.0);
        soft[0] = -soft[0];
        assert_eq!(sync_distance(&soft, WORD, 0x7F).upright, 0.0);
        let silence = sync_distance(&[0.0f32; 8], WORD, 0x7F);
        assert_eq!(
            silence,
            SyncDistance {
                upright: 3.5,
                inverted: 3.5
            }
        );
        let mut lone = [0.0f32; 8];
        lone[7] = -5.0;
        assert_eq!(sync_distance(&lone, WORD, 0xFF).upright, 3.5);
    }
}

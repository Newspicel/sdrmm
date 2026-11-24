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
    let unsure = erased as f32 / 2.0;
    let scale = if total > 0.0 {
        heard as f32 / total
    } else {
        0.0
    };
    SyncDistance {
        upright: wrong * scale + unsure,
        inverted: (total - wrong) * scale + unsure,
    }
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

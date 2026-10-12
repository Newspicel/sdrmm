use sdrmm_wire::GSM_SYMBOL_RATE_HZ;

pub(crate) const SPS: usize = 2;
pub(crate) const INPUT_RATE_HZ: f64 = GSM_SYMBOL_RATE_HZ * SPS as f64;
pub(crate) const BURST_SYMBOLS: usize = 148;
pub(crate) const FRAME_SYMBOLS: usize = 1_250;
pub(crate) const FRAME_SAMPLES: f64 = (FRAME_SYMBOLS * SPS) as f64;
pub(crate) const SLOT_SAMPLES: f64 = FRAME_SAMPLES / 8.0;
pub(crate) const TAIL_SYMBOLS: usize = 3;
pub(crate) const HALF_DATA: usize = 57;
pub(crate) const BLOCK_BURSTS: usize = 4;
pub(crate) const BURST_DATA: usize = 2 * HALF_DATA;
pub(crate) const NORMAL_TRAINING_AT: usize = 61;
pub(crate) const SECOND_HALF_AT: usize = 88;
pub(crate) const SYNC_TRAINING_AT: usize = 42;
pub(crate) const SYNC_DATA: usize = 39;
pub(crate) const SYNC_SECOND_HALF_AT: usize = 106;
pub(crate) const HYPERFRAME: u32 = 2_715_648;

const NORMAL_TSC: [u32; 8] = [
    0x097_0897, 0x0B7_78B7, 0x10E_E90E, 0x11E_D11E, 0x06B_906B, 0x13A_C13A, 0x29F_629F, 0x3BC_4BBC,
];
const NORMAL_TSC_BITS: usize = 26;
const NORMAL_CORE: (usize, usize) = (5, 21);
const SYNC_SEQUENCE: u64 = 0xB962_040F_2D45_761B;
const SYNC_BITS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Training {
    bits: u64,
    len: usize,
    pub(crate) at: usize,
    core: (usize, usize),
}

impl Training {
    pub(crate) fn normal(tsc: u8) -> Self {
        Self {
            bits: u64::from(NORMAL_TSC[usize::from(tsc & 7)]),
            len: NORMAL_TSC_BITS,
            at: NORMAL_TRAINING_AT,
            core: NORMAL_CORE,
        }
    }

    pub(crate) fn sync() -> Self {
        Self {
            bits: SYNC_SEQUENCE,
            len: SYNC_BITS,
            at: SYNC_TRAINING_AT,
            core: (0, SYNC_BITS),
        }
    }

    pub(crate) const fn core(&self) -> core::ops::Range<usize> {
        self.core.0..self.core.1
    }

    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn bit(&self, index: usize) -> bool {
        self.bits >> (self.len - 1 - index) & 1 == 1
    }

    pub(crate) fn symbol(&self, index: usize) -> f32 {
        if self.bit(index) { -1.0 } else { 1.0 }
    }

    pub(crate) fn known(&self, position: usize) -> Option<bool> {
        if !(TAIL_SYMBOLS..BURST_SYMBOLS - TAIL_SYMBOLS).contains(&position) {
            return Some(false);
        }
        position
            .checked_sub(self.at)
            .filter(|&index| index < self.len)
            .map(|index| self.bit(index))
    }
}

pub(crate) fn normal_data(soft: &[f32; BURST_SYMBOLS], out: &mut [f32; BURST_DATA]) {
    out[..HALF_DATA].copy_from_slice(&soft[TAIL_SYMBOLS..TAIL_SYMBOLS + HALF_DATA]);
    out[HALF_DATA..].copy_from_slice(&soft[SECOND_HALF_AT..SECOND_HALF_AT + HALF_DATA]);
}

pub(crate) fn sync_data(soft: &[f32; BURST_SYMBOLS], out: &mut [f32; 2 * SYNC_DATA]) {
    out[..SYNC_DATA].copy_from_slice(&soft[TAIL_SYMBOLS..TAIL_SYMBOLS + SYNC_DATA]);
    out[SYNC_DATA..].copy_from_slice(&soft[SYNC_SECOND_HALF_AT..SYNC_SECOND_HALF_AT + SYNC_DATA]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_training_sequences_wrap_their_core() {
        for tsc in 0..8 {
            let training = Training::normal(tsc);
            for k in 0..5 {
                assert_eq!(training.bit(k), training.bit(k + 16), "tsc {tsc}");
                assert_eq!(training.bit(21 + k), training.bit(5 + k), "tsc {tsc}");
            }
        }
    }

    #[test]
    fn the_core_correlates_to_a_single_peak_within_five_symbols() {
        for tsc in 0..8 {
            let training = Training::normal(tsc);
            for lag in -5i32..=5 {
                let sum: f32 = training
                    .core()
                    .map(|i| training.symbol(i) * training.symbol((i as i32 + lag) as usize))
                    .sum();
                let expected = if lag == 0 { 16.0 } else { 0.0 };
                assert_eq!(sum, expected, "tsc {tsc} lag {lag}");
            }
        }
    }

    #[test]
    fn known_positions_cover_tails_and_training() {
        let training = Training::normal(0);
        assert_eq!(training.known(0), Some(false));
        assert_eq!(training.known(147), Some(false));
        assert_eq!(training.known(3), None);
        assert_eq!(training.known(61), Some(false));
        assert_eq!(training.known(63), Some(true));
        assert_eq!(training.known(87), None);
    }
}

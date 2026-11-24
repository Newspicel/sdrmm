use std::ops::RangeInclusive;

use sdrmm_dsp::{DcBlocker, RealDecimator, SoftFrame, design_lowpass};

use super::{G3RUH_BAUD, lane::Lane};
use crate::soft::soft_xor;

const CUTOFF_HZ: f64 = 7_200.0;
const TAPS: usize = 15;
/// The scrambler's taps, `x^17 + x^12 + 1`, as how many bits back each one reads.
const NEAR_TAP: usize = 12;
const FAR_TAP: usize = 17;

/// The G3RUH descrambler on soft line levels: each bit is the soft XOR of the level and the
/// levels at the taps, so one weak level marks every bit it touches as weak.
struct SoftDescrambler {
    history: [f32; FAR_TAP],
    next: usize,
}

impl SoftDescrambler {
    const fn new() -> Self {
        Self {
            history: [0.0; FAR_TAP],
            next: 0,
        }
    }

    fn back(&self, bits: usize) -> f32 {
        self.history[(self.next + FAR_TAP - bits) % FAR_TAP]
    }

    fn push(&mut self, level: f32) -> f32 {
        let out = soft_xor(soft_xor(level, self.back(NEAR_TAP)), self.back(FAR_TAP));
        self.history[self.next] = level;
        self.next = (self.next + 1) % FAR_TAP;
        out
    }
}

pub(super) struct G3ruh {
    lowpass: RealDecimator,
    dc: DcBlocker,
    filtered: Vec<f32>,
    lane: Lane,
    descrambler: SoftDescrambler,
}

impl G3ruh {
    pub(super) fn new(rate: f64, bytes: &RangeInclusive<usize>) -> Self {
        Self {
            lowpass: RealDecimator::new(&design_lowpass(TAPS, CUTOFF_HZ / rate), 1),
            dc: DcBlocker::new(),
            filtered: Vec::new(),
            lane: Lane::new(rate, G3RUH_BAUD, 1.0, bytes),
            descrambler: SoftDescrambler::new(),
        }
    }

    pub(super) fn frames(
        &mut self,
        discriminated: &[f32],
        mut levels: Option<&mut Vec<f32>>,
        heard: &mut impl FnMut(SoftFrame<'_>),
    ) {
        self.lowpass.process(discriminated, &mut self.filtered);
        self.dc.process(&mut self.filtered);
        for &s in &self.filtered {
            if let Some(level) = self.lane.sync.push_soft(s) {
                if let Some(levels) = levels.as_deref_mut() {
                    levels.push(level);
                }
                self.lane.push_level(self.descrambler.push(level), heard);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_dsp::{Descrambler, Scrambler};

    use super::*;

    #[test]
    fn the_soft_descrambler_decides_like_the_hard_one() {
        let mut scrambler = Scrambler::g3ruh();
        let (mut hard, mut soft) = (Descrambler::g3ruh(), SoftDescrambler::new());
        for index in 0..400u32 {
            let line = scrambler.push(index.count_ones() % 3 == 0);
            let level = if line {
                1.0 + index as f32
            } else {
                -1.0 - index as f32
            };
            let (decided, value) = (hard.push(line), soft.push(level));
            if index >= FAR_TAP as u32 {
                assert_eq!(value > 0.0, decided, "bit {index}");
                assert_eq!(value.abs(), 1.0 + (index - FAR_TAP as u32) as f32);
            }
        }
    }
}

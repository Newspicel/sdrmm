use std::ops::RangeInclusive;

use sdrmm_dsp::{DcBlocker, Descrambler, RealDecimator, design_lowpass};

use super::{G3RUH_BAUD, lane::Lane};

const CUTOFF_HZ: f64 = 7_200.0;
const TAPS: usize = 15;

pub(super) struct G3ruh {
    lowpass: RealDecimator,
    dc: DcBlocker,
    filtered: Vec<f32>,
    lane: Lane,
    descrambler: Descrambler,
}

impl G3ruh {
    pub(super) fn new(rate: f64, bytes: &RangeInclusive<usize>) -> Self {
        Self {
            lowpass: RealDecimator::new(&design_lowpass(TAPS, CUTOFF_HZ / rate), 1),
            dc: DcBlocker::new(),
            filtered: Vec::new(),
            lane: Lane::new(rate, G3RUH_BAUD, 1.0, bytes),
            descrambler: Descrambler::g3ruh(),
        }
    }

    pub(super) fn frames(
        &mut self,
        discriminated: &[f32],
        mut levels: Option<&mut Vec<f32>>,
        heard: &mut impl FnMut(&[u8]),
    ) {
        self.lowpass.process(discriminated, &mut self.filtered);
        self.dc.process(&mut self.filtered);
        for &s in &self.filtered {
            if let Some(level) = self.lane.sync.push_soft(s) {
                if let Some(levels) = levels.as_deref_mut() {
                    levels.push(level);
                }
                self.lane
                    .push_level(self.descrambler.push(level >= 0.0), heard);
            }
        }
    }
}

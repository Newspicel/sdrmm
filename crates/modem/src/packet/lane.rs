use std::ops::RangeInclusive;

use sdrmm_dsp::{BitSync, HdlcDeframer, SoftFrame};

use crate::symbolcode::{NrziConvention, SoftNrziDecoder};

pub(super) struct Lane {
    pub(super) sync: BitSync,
    nrzi: SoftNrziDecoder,
    deframer: HdlcDeframer,
    pub(super) space_gain: f32,
}

impl Lane {
    pub(super) fn new(
        rate: f64,
        baud: f64,
        space_gain: f32,
        bytes: &RangeInclusive<usize>,
    ) -> Self {
        Self {
            sync: BitSync::new(rate, baud),
            nrzi: SoftNrziDecoder::new(NrziConvention::TransitionOnZero),
            deframer: HdlcDeframer::new(*bytes.start(), *bytes.end()),
            space_gain,
        }
    }

    pub(super) fn push(&mut self, sample: f32, heard: &mut impl FnMut(SoftFrame<'_>)) {
        if let Some(level) = self.sync.push_soft(sample) {
            self.push_level(level, heard);
        }
    }

    pub(super) fn push_watched(
        &mut self,
        sample: f32,
        levels: &mut Vec<f32>,
        heard: &mut impl FnMut(SoftFrame<'_>),
    ) {
        if let Some(level) = self.sync.push_soft(sample) {
            levels.push(level);
            self.push_level(level, heard);
        }
    }

    /// Takes one soft line level, mark positive.
    pub(super) fn push_level(&mut self, level: f32, heard: &mut impl FnMut(SoftFrame<'_>)) {
        if let Some(frame) = self.deframer.push_soft(self.nrzi.decode(level)) {
            heard(frame);
        }
    }
}

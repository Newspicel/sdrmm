use num_complex::Complex;

use super::{
    super::{chip::ChipShaper, framing_overhead_db, pn::PnSequence},
    MAX_CHIPS,
};

#[derive(Clone, Debug)]
pub struct DsssParams {
    pub(super) pn: PnSequence,
    pub(super) shaper: ChipShaper,
    pub(super) search_group_symbols: usize,
}

impl DsssParams {
    #[must_use]
    pub fn new(pn: PnSequence, shaper: ChipShaper, search_group_symbols: usize) -> Self {
        assert!(
            pn.len() <= MAX_CHIPS,
            "PN period {} exceeds the receive path's {MAX_CHIPS}-chip scratch",
            pn.len()
        );
        assert!(search_group_symbols > 0, "a search group spans ≥ 1 symbol");
        Self {
            pn,
            shaper,
            search_group_symbols,
        }
    }

    #[must_use]
    pub fn pn(&self) -> &PnSequence {
        &self.pn
    }

    #[must_use]
    pub fn shaper(&self) -> &ChipShaper {
        &self.shaper
    }

    #[must_use]
    pub fn chips_per_symbol(&self) -> usize {
        self.pn.len()
    }

    #[must_use]
    pub fn symbol_samples(&self) -> usize {
        self.pn.len() * self.shaper.sps()
    }

    #[must_use]
    pub fn processing_gain_db(&self) -> f64 {
        self.pn.processing_gain_db()
    }

    #[must_use]
    pub fn framing_overhead_db(preamble: usize, payload: usize) -> f64 {
        framing_overhead_db(preamble, payload)
    }

    pub(super) fn spread_into(&self, points: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        let scale = (self.pn.len() as f32).sqrt().recip();
        out.reserve(points.len() * self.pn.len());
        for &point in points {
            for &chip in self.pn.chips() {
                out.push(point * (chip * scale));
            }
        }
    }
}

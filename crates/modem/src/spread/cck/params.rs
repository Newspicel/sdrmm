use super::{
    super::{chip::ChipShaper, framing_overhead_db},
    CHIPS, CckMode,
    codebook::Codebook,
};

#[derive(Clone, Debug)]
pub struct CckParams {
    pub(super) codebook: Codebook,
    pub(super) shaper: ChipShaper,
    pub(super) search_group_symbols: usize,
}

impl CckParams {
    #[must_use]
    pub fn new(mode: CckMode, shaper: ChipShaper, search_group_symbols: usize) -> Self {
        assert!(search_group_symbols > 0, "a search group spans ≥ 1 symbol");
        Self {
            codebook: Codebook::new(mode),
            shaper,
            search_group_symbols,
        }
    }

    #[must_use]
    pub fn codebook(&self) -> &Codebook {
        &self.codebook
    }

    #[must_use]
    pub fn shaper(&self) -> &ChipShaper {
        &self.shaper
    }

    #[must_use]
    pub fn mode(&self) -> CckMode {
        self.codebook.mode()
    }

    #[must_use]
    pub fn bits_per_symbol(&self) -> usize {
        self.codebook.mode().bits_per_symbol()
    }

    #[must_use]
    pub fn symbol_samples(&self) -> usize {
        CHIPS * self.shaper.sps()
    }

    #[must_use]
    pub fn framing_overhead_db(preamble: usize, payload: usize) -> f64 {
        framing_overhead_db(preamble, payload)
    }
}

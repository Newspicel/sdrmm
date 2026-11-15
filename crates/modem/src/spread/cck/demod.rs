use num_complex::Complex;

use super::{
    super::{
        chip::{ChipTiming, TIMING_BW, find_burst},
        dsss::Acquisition,
    },
    CHIPS, MAX_BITS, MAX_WORDS,
    params::CckParams,
    tracker::PhaseTracker,
};
use crate::{constellation::demap, linear::PhaseAnchor, soft::Llr};

#[derive(Clone, Debug)]
pub struct CckDemod {
    params: CckParams,
    filtered: Vec<Complex<f32>>,
    known: Vec<Complex<f32>>,
    fitted: Vec<Complex<f32>>,
    expected: Vec<Complex<f32>>,
    acquisition: Option<Acquisition>,
}

impl CckDemod {
    #[must_use]
    pub fn new(params: CckParams) -> Self {
        Self {
            params,
            filtered: Vec::new(),
            known: Vec::new(),
            fitted: Vec::new(),
            expected: Vec::new(),
            acquisition: None,
        }
    }

    #[must_use]
    pub fn params(&self) -> &CckParams {
        &self.params
    }

    #[must_use]
    pub fn acquisition(&self) -> Option<Acquisition> {
        self.acquisition
    }

    pub fn acquire(
        &mut self,
        wave: &[Complex<f32>],
        preamble: &[u32],
        search: usize,
    ) -> Option<Acquisition> {
        self.acquisition = None;
        if preamble.len() < 2 {
            return None;
        }
        self.params.shaper.matched(wave, &mut self.filtered);

        self.known.clear();
        self.params
            .codebook
            .push_chips(preamble, 0, &mut self.known);
        let group_chips = self.params.search_group_symbols * CHIPS;
        let origin = find_burst(
            &self.params.shaper,
            &self.filtered,
            &self.known,
            group_chips,
            search,
        )?;

        self.fitted.clear();
        self.expected.clear();
        let mut bank = [Complex::new(0.0, 0.0); MAX_WORDS];
        let bank = &mut bank[..self.params.codebook.words().len()];
        for (k, &label) in preamble.iter().enumerate() {
            self.bank_at(origin, k, bank);
            self.fitted.push(bank[(label >> 2) as usize % bank.len()]);
            self.expected.push(self.params.codebook.reference(label, k));
        }
        let anchor = PhaseAnchor::fit_gain_only(&self.fitted, &self.expected).ok()?;
        anchor.correct_block(0, &mut self.fitted);
        let noise_var = demap::noise_var_from_known(&self.fitted, &self.expected);
        let acquisition = Acquisition {
            origin,
            anchor,
            noise_var,
        };
        self.acquisition = Some(acquisition);
        Some(acquisition)
    }

    pub fn bank(&self, symbol: usize, out: &mut [Complex<f32>]) {
        let Some(acquisition) = self.acquisition else {
            out.fill(Complex::new(0.0, 0.0));
            return;
        };
        self.bank_at(acquisition.origin, symbol, out);
        for slot in out.iter_mut() {
            *slot = acquisition.anchor.correct(symbol, *slot);
        }
    }

    fn tracked_bank(&self, timing: &ChipTiming, symbol: usize, out: &mut [Complex<f32>]) {
        let Some(acquisition) = self.acquisition else {
            out.fill(Complex::new(0.0, 0.0));
            return;
        };
        let mut chips = [Complex::new(0.0, 0.0); CHIPS];
        let origin = acquisition.origin as f64 + timing.offset();
        self.params
            .shaper
            .block_at(&self.filtered, origin, symbol * CHIPS, &mut chips);
        self.params.codebook.correlate(&chips, out);
        for slot in out.iter_mut() {
            *slot = acquisition.anchor.correct(symbol, *slot);
        }
    }

    fn steer(&self, timing: &mut ChipTiming, label: u32, symbol: usize) {
        let Some(acquisition) = self.acquisition else {
            return;
        };
        let mut word = [Complex::new(0.0, 0.0); CHIPS];
        self.params.codebook.chips(label, symbol, &mut word);
        let energy = |offset: f64| {
            let mut chips = [Complex::new(0.0, 0.0); CHIPS];
            self.params.shaper.block_at(
                &self.filtered,
                acquisition.origin as f64 + offset,
                symbol * CHIPS,
                &mut chips,
            );
            chips
                .iter()
                .zip(&word)
                .map(|(&y, &w)| y * w.conj())
                .sum::<Complex<f32>>()
                .norm_sqr()
        };
        timing.update(energy(timing.early()), energy(timing.late()));
    }

    fn bank_at(&self, origin: usize, symbol: usize, out: &mut [Complex<f32>]) {
        let mut chips = [Complex::new(0.0, 0.0); CHIPS];
        self.params
            .shaper
            .block(&self.filtered, origin, symbol * CHIPS, &mut chips);
        self.params.codebook.correlate(&chips, out);
    }

    pub fn demodulate(&self, preamble_symbols: usize, symbols: usize, out: &mut Vec<u32>) {
        if self.acquisition.is_none() {
            return;
        }
        let mut bank = [Complex::new(0.0, 0.0); MAX_WORDS];
        let bank = &mut bank[..self.params.codebook.words().len()];
        let mut tracker = PhaseTracker::new();
        let mut timing = ChipTiming::new(self.params.shaper.sps(), TIMING_BW);
        out.reserve(symbols);
        for k in 0..symbols {
            let index = preamble_symbols + k;
            self.tracked_bank(&timing, index, bank);
            tracker.derotate(bank);
            let label = self.params.codebook.decide(bank, index);
            tracker.follow(bank, label, &self.params.codebook, index);
            self.steer(&mut timing, label, index);
            out.push(label);
        }
    }

    pub fn llrs(&self, preamble_symbols: usize, symbols: usize, out: &mut Vec<Llr>) {
        let Some(acquisition) = self.acquisition else {
            return;
        };
        let bits = self.params.bits_per_symbol();
        let mut bank = [Complex::new(0.0, 0.0); MAX_WORDS];
        let bank = &mut bank[..self.params.codebook.words().len()];
        let mut symbol_llrs = [Llr(0.0); MAX_BITS];
        let symbol_llrs = &mut symbol_llrs[..bits];
        let noise_var = acquisition.noise_var.max(f64::MIN_POSITIVE);
        let mut tracker = PhaseTracker::new();
        let mut timing = ChipTiming::new(self.params.shaper.sps(), TIMING_BW);
        out.reserve(symbols * bits);
        for k in 0..symbols {
            let index = preamble_symbols + k;
            self.tracked_bank(&timing, index, bank);
            tracker.derotate(bank);
            self.params
                .codebook
                .llrs(bank, index, noise_var, symbol_llrs);
            let label = self.params.codebook.decide(bank, index);
            tracker.follow(bank, label, &self.params.codebook, index);
            self.steer(&mut timing, label, index);
            out.extend_from_slice(symbol_llrs);
        }
    }
}

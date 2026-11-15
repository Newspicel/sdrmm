use num_complex::Complex;
use sdrmm_dsp::math::complex::{narrow, widen};

use super::{
    super::chip::{ChipTiming, TIMING_BW, find_burst, sample_at},
    params::DsssParams,
};
use crate::{
    constellation::{Constellation, demap},
    linear::{CarrierLoop, PhaseAnchor, PhaseDetector},
    soft::Llr,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Acquisition {
    pub origin: usize,
    pub anchor: PhaseAnchor,
    pub noise_var: f64,
}

#[derive(Clone, Debug)]
pub struct DsssDemod {
    params: DsssParams,
    filtered: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    acquisition: Option<Acquisition>,
    tracking: Option<(Constellation, f64)>,
}

impl DsssDemod {
    #[must_use]
    pub fn new(params: DsssParams) -> Self {
        Self {
            params,
            filtered: Vec::new(),
            scratch: Vec::new(),
            acquisition: None,
            tracking: None,
        }
    }

    #[must_use]
    pub fn with_tracking(mut self, table: Constellation, loop_bw: f64) -> Self {
        self.tracking = Some((table, loop_bw));
        self
    }

    #[must_use]
    pub fn params(&self) -> &DsssParams {
        &self.params
    }

    #[must_use]
    pub fn acquisition(&self) -> Option<Acquisition> {
        self.acquisition
    }

    pub fn acquire(
        &mut self,
        wave: &[Complex<f32>],
        preamble: &[Complex<f32>],
        search: usize,
    ) -> Option<Acquisition> {
        self.acquisition = None;
        if preamble.len() < 2 {
            return None;
        }
        self.params.shaper.matched(wave, &mut self.filtered);

        self.scratch.clear();
        self.params.spread_into(preamble, &mut self.scratch);
        let group_chips = self.params.search_group_symbols * self.params.chips_per_symbol();
        let origin = find_burst(
            &self.params.shaper,
            &self.filtered,
            &self.scratch,
            group_chips,
            search,
        )?;

        self.scratch.clear();
        for symbol in 0..preamble.len() {
            let point = self.despread(origin, symbol)?;
            self.scratch.push(point);
        }
        let anchor = PhaseAnchor::fit_gain_only(&self.scratch, preamble).ok()?;
        anchor.correct_block(0, &mut self.scratch);
        let noise_var = demap::noise_var_from_known(&self.scratch, preamble);
        let acquisition = Acquisition {
            origin,
            anchor,
            noise_var,
        };
        self.acquisition = Some(acquisition);
        Some(acquisition)
    }

    pub fn genie(&mut self, wave: &[Complex<f32>], acquisition: Acquisition) {
        self.params.shaper.matched(wave, &mut self.filtered);
        self.acquisition = Some(acquisition);
    }

    pub fn demodulate(&self, preamble_symbols: usize, symbols: usize, out: &mut Vec<Complex<f32>>) {
        let Some(acquisition) = self.acquisition else {
            return;
        };
        out.reserve(symbols);
        let Some((table, bw)) = &self.tracking else {
            for k in 0..symbols {
                let index = preamble_symbols + k;
                let Some(raw) = self.despread(acquisition.origin, index) else {
                    return;
                };
                out.push(acquisition.anchor.correct(index, raw));
            }
            return;
        };
        let mut carrier = CarrierLoop::new(PhaseDetector::DecisionDirected, *bw);
        let mut timing = ChipTiming::new(self.params.shaper.sps(), TIMING_BW);
        let origin = acquisition.origin as f64;
        for k in 0..symbols {
            let index = preamble_symbols + k;
            let raw = self.despread_at(origin + timing.offset(), index);
            let early = self.despread_at(origin + timing.early(), index).norm_sqr();
            let late = self.despread_at(origin + timing.late(), index).norm_sqr();
            timing.update(early, late);
            out.push(carrier.advance(acquisition.anchor.correct(index, raw), table));
        }
    }

    #[must_use]
    pub fn despread_at(&self, origin: f64, symbol: usize) -> Complex<f32> {
        let n = self.params.chips_per_symbol();
        let sps = self.params.shaper.sps();
        let mut acc = Complex::new(0.0f64, 0.0);
        for (c, &code) in self.params.pn.chips().iter().enumerate() {
            let y = sample_at(&self.filtered, origin + ((symbol * n + c) * sps) as f64);
            acc += widen(y) * f64::from(code);
        }
        narrow(acc * (n as f64).sqrt().recip())
    }

    #[must_use]
    pub fn despread(&self, origin: usize, symbol: usize) -> Option<Complex<f32>> {
        let n = self.params.chips_per_symbol();
        let sps = self.params.shaper.sps();
        let chips = self.params.pn.chips();
        let mut re = 0.0f64;
        let mut im = 0.0f64;
        for (c, &code) in chips.iter().enumerate() {
            let &y = self.filtered.get(origin + (symbol * n + c) * sps)?;
            re += f64::from(code) * f64::from(y.re);
            im += f64::from(code) * f64::from(y.im);
        }
        let scale = (n as f64).sqrt().recip();
        Some(Complex::new((re * scale) as f32, (im * scale) as f32))
    }

    pub fn llrs(&self, points: &[Complex<f32>], table: &Constellation, out: &mut [Llr]) {
        let bits = table.bits_per_symbol();
        assert_eq!(
            out.len(),
            points.len() * bits,
            "one LLR slot per payload bit"
        );
        let Some(acquisition) = self.acquisition else {
            out.fill(Llr(0.0));
            return;
        };
        let noise_var = acquisition.noise_var.max(f64::MIN_POSITIVE);
        for (point, slot) in points.iter().zip(out.chunks_exact_mut(bits)) {
            demap::max_log_llrs(*point, table, noise_var, slot);
        }
    }
}

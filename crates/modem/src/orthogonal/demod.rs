use num_complex::Complex;

use super::{
    filterbank::ToneBank,
    params::{MAX_TONES, MfskParams},
};
use crate::{
    constellation::demap::energy_llrs,
    soft::{Llr, argmax},
};

const SCRATCH: usize = MAX_TONES;

/// Weight of an offset's own energy against its earlier neighbour's in the timing score.
/// Above 1 so a lone peak beats its later neighbour; finite so a two-sample plateau picks its
/// later sample.
const OWN_WEIGHT: f64 = 2.0;

#[derive(Clone, Debug)]
pub struct MfskDemod {
    params: MfskParams,
    bank: ToneBank,
}

impl MfskDemod {
    #[must_use]
    pub fn new(params: MfskParams) -> Self {
        let bank = ToneBank::new(&params);
        Self { params, bank }
    }

    #[must_use]
    pub fn params(&self) -> &MfskParams {
        &self.params
    }

    #[must_use]
    pub fn m(&self) -> usize {
        self.params.m()
    }

    #[must_use]
    pub fn bits_per_symbol(&self) -> u32 {
        self.params.bits_per_symbol()
    }

    #[must_use]
    pub fn energies(
        &self,
        iq: &[Complex<f32>],
        offset: usize,
        symbol: usize,
        out: &mut [f32],
    ) -> bool {
        self.bank
            .energies(iq, offset + symbol * self.params.window(), out)
    }

    /// The sample in `0..window` where a symbol starts, judged over up to `symbols` symbols.
    ///
    /// A continuous-phase tone has its phase knot on a sample, so it stays coherent over
    /// `window + 1` samples and the true start and the sample before it read the same energy.
    /// When the start is 0 that twin sits at `window - 1`, a whole symbol late. So a coarse pass
    /// only finds the plateau, and a fine pass picks its later sample from neighbours scored
    /// over the same symbols, never across the wrap. Scoring starts one symbol in so every
    /// candidate has those neighbours, and every candidate counts the same number of windows.
    /// `None` when `iq` holds under three symbols or `symbols` is 0.
    #[must_use]
    pub fn estimate_offset(&self, iq: &[Complex<f32>], symbols: usize) -> Option<usize> {
        let window = self.params.window();
        let symbols = symbols.min(iq.len().saturating_sub(2 * window) / window);
        if symbols == 0 {
            return None;
        }
        let coarse =
            strongest((window..2 * window).map(|at| (at, self.timing_energy(iq, at, symbols))));
        let fine = strongest((coarse - 1..=coarse + 1).map(|at| {
            let own = self.timing_energy(iq, at, symbols);
            (
                at,
                OWN_WEIGHT * own + self.timing_energy(iq, at - 1, symbols),
            )
        }));
        Some(fine % window)
    }

    fn timing_energy(&self, iq: &[Complex<f32>], offset: usize, symbols: usize) -> f64 {
        let mut energies = [0.0f32; SCRATCH];
        let energies = &mut energies[..self.m()];
        let mut sum = 0.0f64;
        for symbol in 0..symbols {
            if !self.energies(iq, offset, symbol, energies) {
                break;
            }
            sum += f64::from(energies.iter().copied().fold(0.0f32, f32::max));
        }
        sum
    }

    pub fn demodulate(
        &self,
        iq: &[Complex<f32>],
        offset: usize,
        symbols: usize,
        out: &mut Vec<u8>,
    ) -> usize {
        let mut energies = [0.0f32; SCRATCH];
        let energies = &mut energies[..self.m()];
        out.reserve(symbols);
        for symbol in 0..symbols {
            if !self.energies(iq, offset, symbol, energies) {
                return symbol;
            }
            out.push(argmax(energies));
        }
        symbols
    }

    pub fn llrs(
        &self,
        iq: &[Complex<f32>],
        offset: usize,
        symbols: usize,
        noise_var: f64,
        out: &mut Vec<Llr>,
    ) -> usize {
        let bits = self.bits_per_symbol() as usize;
        let mut energies = [0.0f32; SCRATCH];
        let mut symbol_llrs = [Llr(0.0); SCRATCH];
        let energies = &mut energies[..self.m()];
        let symbol_llrs = &mut symbol_llrs[..bits];
        out.reserve(symbols * bits);
        for symbol in 0..symbols {
            if !self.energies(iq, offset, symbol, energies) {
                return symbol;
            }
            energy_llrs(energies, noise_var, symbol_llrs);
            out.extend_from_slice(symbol_llrs);
        }
        symbols
    }
}

fn strongest(scores: impl Iterator<Item = (usize, f64)>) -> usize {
    scores
        .fold((0, f64::NEG_INFINITY), |best, next| {
            if next.1 > best.1 { next } else { best }
        })
        .0
}

#[must_use]
pub fn noise_var_from_energies(energies: &[f32], m: usize) -> f64 {
    assert!(m > 1, "a noise estimate needs a bin the signal is not in");
    assert!(
        !energies.is_empty() && energies.len().is_multiple_of(m),
        "energies must be a whole number of {m}-tone symbols"
    );
    let mut sum = 0.0f64;
    for symbol in energies.chunks_exact(m) {
        let peak = symbol.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        sum += symbol.iter().map(|&e| f64::from(e)).sum::<f64>() - f64::from(peak);
    }
    sum / (energies.len() - energies.len() / m) as f64
}

#[cfg(test)]
mod tests {
    use super::{
        super::{modulator::TonePhase, testutil::wave as modulate},
        *,
    };
    use crate::testutil::{add_noise, draws};

    const SPS: f64 = 10.0;

    fn params(m: usize) -> MfskParams {
        MfskParams::orthogonal(m, SPS)
    }

    #[test]
    fn every_alphabet_round_trips_clean() {
        for m in [2usize, 4, 8, 16] {
            let p = MfskParams::orthogonal(m, (2 * m) as f64);
            let symbols = draws(200, m, 0x9e37_79b9);
            let wave = modulate(&p, TonePhase::Continuous, &symbols);
            let mut out = Vec::new();
            MfskDemod::new(p).demodulate(&wave, 0, symbols.len(), &mut out);
            assert_eq!(out, symbols, "M = {m}");
        }
    }

    #[test]
    fn both_phase_policies_decode_identically() {
        let p = params(4);
        let symbols = draws(2_000, 4, 0x9e37_79b9);
        let demod = MfskDemod::new(p.clone());
        let errors = |policy| {
            let mut wave = modulate(&p, policy, &symbols);
            add_noise(&mut wave, 0x0f5c, 1.4);
            let mut out = Vec::new();
            demod.demodulate(&wave, 0, symbols.len(), &mut out);
            out.iter().zip(&symbols).filter(|(a, b)| a != b).count()
        };
        let (continuous, independent) = (
            errors(TonePhase::Continuous),
            errors(TonePhase::Independent),
        );
        assert!(continuous > 20 && independent > 20, "too clean to compare");
        let ratio = continuous as f64 / independent as f64;
        assert!(
            (0.7..1.4).contains(&ratio),
            "continuous {continuous} vs independent {independent} errors"
        );
    }

    const POLICIES: [TonePhase; 2] = [TonePhase::Continuous, TonePhase::Independent];

    fn burst(p: &MfskParams, policy: TonePhase, symbols: &[u8], shift: usize) -> Vec<Complex<f32>> {
        let mut wave = vec![Complex::new(0.0, 0.0); shift];
        wave.extend(modulate(p, policy, symbols));
        wave
    }

    #[test]
    fn the_timing_estimate_finds_every_shift_for_any_data() {
        for (policy, m, sps) in POLICIES
            .into_iter()
            .flat_map(|policy| [2usize, 4, 8].map(|m| (policy, m)))
            .flat_map(|(policy, m)| [SPS, 16.0].map(|sps| (policy, m, sps)))
        {
            let p = MfskParams::orthogonal(m, sps);
            let demod = MfskDemod::new(p.clone());
            for seed in 0..20 {
                let symbols = draws(64, m, seed);
                for shift in 0..p.window() {
                    let wave = burst(&p, policy, &symbols, shift);
                    let case = format!("{policy:?} M = {m}, sps {sps}, seed {seed}, shift {shift}");
                    assert_eq!(demod.estimate_offset(&wave, 32), Some(shift), "{case}");
                    let mut out = Vec::new();
                    demod.demodulate(&wave, shift, symbols.len(), &mut out);
                    assert_eq!(out, symbols, "{case}");
                }
            }
        }
    }

    /// A continuous-phase tone gives up its timing only where the tone changes, one sample
    /// past its knot, so in noise only alphabets that change tone often time exactly.
    #[test]
    fn the_timing_estimate_holds_at_10_db_es_n0() {
        let cases = [
            (TonePhase::Continuous, 8usize),
            (TonePhase::Independent, 2),
            (TonePhase::Independent, 4),
            (TonePhase::Independent, 8),
        ];
        for (policy, m) in cases {
            let p = params(m);
            let demod = MfskDemod::new(p.clone());
            for seed in 0..20 {
                let symbols = draws(64, m, seed);
                for shift in 0..p.window() {
                    let mut wave = burst(&p, policy, &symbols, shift);
                    add_noise(&mut wave, seed ^ 0x5eed, 1.0);
                    let got = demod.estimate_offset(&wave, 32);
                    assert_eq!(got, Some(shift), "{policy:?} M = {m}, seed {seed}");
                }
            }
        }
    }

    #[test]
    fn a_burst_cut_mid_symbol_still_times_exactly() {
        let p = params(4);
        let window = p.window();
        let demod = MfskDemod::new(p.clone());
        for seed in 0..20 {
            let symbols = draws(6, 4, seed);
            for shift in 0..window {
                let wave = burst(&p, TonePhase::Continuous, &symbols, shift);
                let cut = &wave[..shift + 5 * window + window / 2];
                assert_eq!(
                    demod.estimate_offset(cut, usize::MAX),
                    Some(shift),
                    "seed {seed}"
                );
            }
        }
    }

    #[test]
    fn a_buffer_under_three_symbols_gives_no_timing() {
        let p = params(4);
        let window = p.window();
        let demod = MfskDemod::new(p.clone());
        let wave = burst(&p, TonePhase::Continuous, &[1, 2, 3], 0);
        assert_eq!(demod.estimate_offset(&wave[..3 * window - 1], 8), None);
        assert_eq!(demod.estimate_offset(&wave[..3 * window], 8), Some(0));
        assert_eq!(demod.estimate_offset(&wave, 0), None);
    }

    #[test]
    fn a_truncated_buffer_decides_only_the_whole_symbols() {
        let p = params(8);
        let window = p.window();
        let symbols = draws(16, 8, 0x9e37_79b9);
        let wave = modulate(&p, TonePhase::Continuous, &symbols);
        let demod = MfskDemod::new(p);
        let short = &wave[..10 * window + 1];
        let mut out = Vec::new();
        assert_eq!(demod.demodulate(short, 0, 16, &mut out), 10);
        assert_eq!(out.len(), 10);
        let mut llrs = Vec::new();
        assert_eq!(demod.llrs(short, 0, 16, 1.0, &mut llrs), 10);
        assert_eq!(llrs.len(), 10 * demod.bits_per_symbol() as usize);
    }

    #[test]
    fn a_half_symbol_offset_loses_most_of_the_energy() {
        let p = params(8);
        let symbols = draws(64, 8, 0x9e37_79b9);
        let wave = modulate(&p, TonePhase::Continuous, &symbols);
        let demod = MfskDemod::new(p);
        let mut energies = [0.0f32; 8];
        let peak_at = |offset: usize, energies: &mut [f32]| {
            let mut sum = 0.0f64;
            for symbol in 0..60 {
                assert!(demod.energies(&wave, offset, symbol, energies));
                sum += f64::from(energies.iter().copied().fold(0.0f32, f32::max));
            }
            sum
        };
        let aligned = peak_at(0, &mut energies);
        let split = peak_at(5, &mut energies);
        assert!(split < 0.6 * aligned, "aligned {aligned}, split {split}");
    }

    #[test]
    fn the_noise_estimate_recovers_a_known_n0() {
        let p = params(8);
        let symbols = draws(3_000, 8, 0x9e37_79b9);
        let mut wave = modulate(&p, TonePhase::Continuous, &symbols);
        add_noise(&mut wave, 0x9711, 2.0);
        let demod = MfskDemod::new(p);
        let mut energies = vec![0.0f32; 8 * symbols.len()];
        for symbol in 0..symbols.len() {
            let at = symbol * 8;
            assert!(demod.energies(&wave, 0, symbol, &mut energies[at..at + 8]));
        }
        let estimate = noise_var_from_energies(&energies, 8);
        assert!(
            (estimate - 2.0).abs() < 0.1,
            "estimated N0 {estimate} vs 2.0"
        );
    }

    #[test]
    fn llr_magnitudes_predict_their_own_error_rate() {
        let p = params(4);
        let symbols = draws(40_000, 4, 0x9e37_79b9);
        let mut wave = modulate(&p, TonePhase::Continuous, &symbols);
        add_noise(&mut wave, 0x11cb, 1.0);
        let demod = MfskDemod::new(p);
        let mut llrs = Vec::new();
        demod.llrs(&wave, 0, symbols.len(), 1.0, &mut llrs);
        let mut bands = [(0u32, 0u32); 4];
        for (i, &llr) in llrs.iter().enumerate() {
            let sent = (symbols[i / 2] >> (i % 2)) & 1 == 1;
            let band = match llr.0.abs() {
                x if x < 1.0 => 0,
                x if x < 2.0 => 1,
                x if x < 4.0 => 2,
                _ => 3,
            };
            bands[band].0 += 1;
            if llr.bit() != sent {
                bands[band].1 += 1;
            }
        }
        for (band, &(count, wrong)) in bands.iter().enumerate() {
            assert!(count > 500, "band {band} saw only {count} bits");
            let measured = f64::from(wrong) / f64::from(count);
            let centre = [0.5f64, 1.5, 3.0, 5.0][band];
            let predicted = 1.0 / (1.0 + centre.exp());
            assert!(
                measured < predicted * 3.0 + 0.02,
                "band {band}: measured {measured}, max-log prediction {predicted}"
            );
        }
    }
}

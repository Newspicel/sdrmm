mod demod;
mod modulator;

use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::{Decibels, math::complex::narrow};

pub use self::{demod::FbmcDemod, modulator::FbmcMod};
use super::allocation::centred_bin;

/// PHYDYAS prototype coefficients for an overlap of four.
const PHYDYAS_K4: [f64; 3] = [0.971_960, std::f64::consts::FRAC_1_SQRT_2, 0.235_147];
const REFERENCE_SUBCARRIERS: usize = 64;
const REFERENCE_ALLOCATED: usize = 48;
const QUARTER_TURNS: [Complex<f32>; 4] = [
    Complex::new(1.0, 0.0),
    Complex::new(0.0, 1.0),
    Complex::new(-1.0, 0.0),
    Complex::new(0.0, -1.0),
];

pub const MAX_SUBCARRIERS: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FbmcParams {
    pub subcarriers: usize,
    pub allocated: usize,
}

impl FbmcParams {
    pub const OVERLAP: usize = 4;

    #[must_use]
    pub fn reference() -> Self {
        Self {
            subcarriers: REFERENCE_SUBCARRIERS,
            allocated: REFERENCE_ALLOCATED,
        }
    }
    #[must_use]
    pub fn prototype_len(&self) -> usize {
        Self::OVERLAP * self.subcarriers
    }

    #[must_use]
    pub fn slot_stride(&self) -> usize {
        self.subcarriers / 2
    }

    #[must_use]
    pub fn frame_samples(&self, points: usize) -> usize {
        let slots = 2 * points / self.allocated;
        (slots.max(1) - 1) * self.slot_stride() + self.prototype_len()
    }

    #[must_use]
    pub fn prototype(&self) -> Vec<f64> {
        assert!(
            (2..=MAX_SUBCARRIERS).contains(&self.subcarriers) && self.subcarriers.is_multiple_of(2),
            "an FBMC bank runs an even 2..={MAX_SUBCARRIERS} subcarriers"
        );
        assert!(
            (1..=self.subcarriers).contains(&self.allocated),
            "an allocation runs 1..={} subcarriers of the bank, got {}",
            self.subcarriers,
            self.allocated
        );
        let len = self.prototype_len();
        let mut p: Vec<f64> = (0..len)
            .map(|n| {
                let mut acc = 1.0;
                for (k, &h) in PHYDYAS_K4.iter().enumerate() {
                    let order = (k + 1) as f64;
                    let sign = if (k + 1) % 2 == 0 { 1.0 } else { -1.0 };
                    acc += 2.0 * sign * h * (TAU * order * n as f64 / len as f64).cos();
                }
                acc
            })
            .collect();
        let scale = p.iter().map(|v| v * v).sum::<f64>().sqrt().recip();
        for v in &mut p {
            *v *= scale;
        }
        p
    }

    #[must_use]
    pub fn bin(&self, index: usize) -> usize {
        centred_bin(index, self.allocated, self.subcarriers)
    }

    #[must_use]
    pub(super) fn kernels(&self) -> Vec<Vec<Complex<f32>>> {
        let p = self.prototype();
        let len = self.prototype_len();
        let delay = (len / 2) as f64;
        (0..self.allocated)
            .map(|index| {
                let m = self.bin(index) as f64;
                (0..len)
                    .map(|u| {
                        let phase = TAU * m * (u as f64 - delay) / self.subcarriers as f64;
                        narrow(Complex::from_polar(p[u], phase))
                    })
                    .collect()
            })
            .collect()
    }

    pub(super) fn oqam_phase(&self, index: usize, slot: usize) -> Complex<f32> {
        let m = self.bin(index);
        let base = QUARTER_TURNS[(m + slot) % QUARTER_TURNS.len()];
        if (m * slot).is_multiple_of(2) {
            base
        } else {
            -base
        }
    }
}

#[must_use]
pub fn prototype_response_db(params: &FbmcParams, offset: f64) -> f64 {
    let p = params.prototype();
    let m = params.subcarriers as f64;
    let at = |f: f64| {
        p.iter()
            .enumerate()
            .map(|(n, &tap)| Complex::from_polar(tap, -TAU * f * n as f64 / m))
            .sum::<Complex<f64>>()
            .norm_sqr()
    };
    (at(offset) / at(0.0)).power_to_db()
}

#[cfg(test)]
mod tests {
    use sdrmm_dsp::math::complex::widen;

    use super::*;
    use crate::testutil::qpsk;

    fn points(count: usize) -> Vec<Complex<f32>> {
        qpsk(count, 0x0_fb3c)
    }

    #[test]
    fn the_phydyas_prototype_is_unit_energy_and_symmetric() {
        let params = FbmcParams::reference();
        let p = params.prototype();
        assert_eq!(p.len(), 256);
        let energy: f64 = p.iter().map(|v| v * v).sum();
        assert!((energy - 1.0).abs() < 1e-12, "energy {energy}");
        for n in 1..p.len() / 2 {
            assert!(
                (p[n] - p[p.len() - n]).abs() < 1e-12,
                "asymmetric at tap {n}"
            );
        }
        let one_away = prototype_response_db(&params, 1.0);
        let two_away = prototype_response_db(&params, 2.0);
        assert!(one_away < -20.0, "one spacing away: {one_away} dB");
        assert!(two_away < -35.0, "two spacings away: {two_away} dB");
    }

    #[test]
    fn a_noiseless_frame_round_trips() {
        let params = FbmcParams::reference();
        let symbols = 16;
        let sent = points(symbols * params.allocated);
        let mut wave = Vec::new();
        FbmcMod::new(params).modulate(&sent, &mut wave);
        assert_eq!(wave.len(), params.frame_samples(sent.len()));
        let mut got = Vec::new();
        FbmcDemod::new(params).demodulate(&wave, symbols, &mut got);
        assert_eq!(got.len(), sent.len());
        let interior = params.allocated * 4..sent.len() - params.allocated * 4;
        let worst = got[interior.clone()]
            .iter()
            .zip(&sent[interior])
            .map(|(a, b)| f64::from((a - b).norm()))
            .fold(0.0f64, f64::max);
        assert!(worst < 0.02, "worst interior error {worst}");
    }

    #[test]
    fn a_frame_cut_short_demodulates_only_its_whole_symbols() {
        let params = FbmcParams::reference();
        let symbols = 16;
        let sent = points(symbols * params.allocated);
        let mut wave = Vec::new();
        FbmcMod::new(params).modulate(&sent, &mut wave);
        let stride = params.slot_stride();
        let cut = &wave[..11 * 2 * stride];
        let mut got = Vec::new();
        let decided = FbmcDemod::new(params).demodulate(cut, symbols, &mut got);
        assert!(decided < symbols, "{decided} of {symbols} symbols");
        assert_eq!(got.len(), decided * params.allocated);
    }

    #[test]
    fn the_frame_carries_the_energy_of_its_points() {
        let params = FbmcParams::reference();
        let symbols = 32;
        let sent = points(symbols * params.allocated);
        let mut wave = Vec::new();
        FbmcMod::new(params).modulate(&sent, &mut wave);
        let wave_energy: f64 = wave.iter().map(|v| f64::from(v.norm_sqr())).sum();
        let point_energy: f64 = sent.iter().map(|v| f64::from(v.norm_sqr())).sum();
        assert!(
            (wave_energy / point_energy - 1.0).abs() < 0.05,
            "waveform {wave_energy}, points {point_energy}"
        );
    }

    #[test]
    fn without_the_half_symbol_offset_the_bank_is_not_orthogonal() {
        let params = FbmcParams::reference();
        let kernels = params.kernels();
        let inner: Complex<f64> = kernels[10]
            .iter()
            .zip(&kernels[11])
            .map(|(a, b)| widen(*a) * widen(*b).conj())
            .sum();
        let phase = params.oqam_phase(10, 0) * params.oqam_phase(11, 0).conj();
        let projected = inner * widen(phase);
        assert!(inner.norm() > 0.1, "neighbours do not overlap: {inner}");
        assert!(
            projected.re.abs() < 1e-3,
            "the real part is not orthogonal: {}",
            projected.re
        );
    }

    #[test]
    fn an_allocation_the_bank_cannot_carry_is_rejected() {
        for allocated in [0usize, 65] {
            let params = FbmcParams {
                subcarriers: 64,
                allocated,
            };
            let panicked = std::panic::catch_unwind(|| params.prototype()).is_err();
            assert!(panicked, "an allocation of {allocated} was accepted");
        }
    }
}

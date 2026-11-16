mod demod;
mod modulator;

use num_complex::Complex;
use sdrmm_dsp::{Decibels, design_lowpass};

pub use self::{demod::UfmcDemod, modulator::UfmcMod};
use super::allocation::{centred_bin, signed_offset};

const REFERENCE_FFT: usize = 128;
const REFERENCE_SUBBANDS: usize = 4;
const REFERENCE_PER_SUBBAND: usize = 12;
const REFERENCE_FILTER_LEN: usize = 33;
/// Prototype transition width times its tap count, the Dolph-Chebyshev rule of thumb.
const TRANSITION_TAPS: f64 = 2.75;
/// The highest cutoff the lowpass designer accepts, just short of Nyquist.
const MAX_CUTOFF: f64 = 0.4999;
const MIN_FILTER_LEN: usize = 3;

pub const MAX_FFT: usize = 4_096;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UfmcParams {
    pub fft: usize,
    pub subbands: usize,
    pub per_subband: usize,
    pub filter_len: usize,
}

impl UfmcParams {
    #[must_use]
    pub fn reference() -> Self {
        Self {
            fft: REFERENCE_FFT,
            subbands: REFERENCE_SUBBANDS,
            per_subband: REFERENCE_PER_SUBBAND,
            filter_len: REFERENCE_FILTER_LEN,
        }
    }

    #[must_use]
    pub fn points(&self) -> usize {
        self.subbands * self.per_subband
    }

    #[must_use]
    pub fn samples(&self) -> usize {
        self.fft + self.filter_len - 1
    }

    #[must_use]
    pub fn overhead_db(&self) -> f64 {
        (self.samples() as f64 / self.fft as f64).power_to_db()
    }

    #[must_use]
    pub fn bin(&self, index: usize) -> usize {
        centred_bin(index, self.points(), self.fft)
    }

    #[must_use]
    pub fn subband_of(&self, index: usize) -> usize {
        index / self.per_subband
    }

    #[must_use]
    pub fn subband_filter(&self, b: usize) -> Vec<Complex<f32>> {
        assert!(
            (1..=MAX_FFT).contains(&self.fft)
                && (MIN_FILTER_LEN..=self.fft + 1).contains(&self.filter_len)
                && self.subbands.is_multiple_of(2),
            "a UFMC symbol runs a 1..={MAX_FFT} transform, 3..={} filter taps and an even subband \
             count; got {} taps over {} subbands of a {}-point transform",
            self.fft + 1,
            self.filter_len,
            self.subbands,
            self.fft
        );
        let half_width = self.per_subband as f64 / 2.0 / self.fft as f64;
        let transition = TRANSITION_TAPS / self.filter_len as f64;
        let prototype = design_lowpass(self.filter_len, (half_width + transition).min(MAX_CUTOFF));
        let first = b * self.per_subband;
        let last = first + self.per_subband - 1;
        let centre = f64::midpoint(self.signed_bin(first), self.signed_bin(last)) / self.fft as f64;
        prototype
            .iter()
            .enumerate()
            .map(|(n, &tap)| {
                Complex::from_polar(tap, (std::f64::consts::TAU * centre * n as f64) as f32)
            })
            .collect()
    }

    fn signed_bin(&self, index: usize) -> f64 {
        signed_offset(index, self.points()) as f64
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_dsp::math::complex::widen;

    use super::*;
    use crate::testutil::qpsk;

    fn points(count: usize) -> Vec<Complex<f32>> {
        qpsk(count, 0x0_fc5e)
    }

    #[test]
    fn the_subcarrier_map_is_symmetric_and_skips_dc() {
        let params = UfmcParams::reference();
        let bins: Vec<usize> = (0..params.points()).map(|k| params.bin(k)).collect();
        assert_eq!(bins.len(), 48);
        assert!(!bins.contains(&0), "DC is allocated");
        let mut sorted = bins.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), bins.len(), "a bin is allocated twice");
        assert_eq!(bins[0], params.fft - 24);
        assert_eq!(bins[47], 24);
    }

    #[test]
    fn the_receiver_round_trips_exactly() {
        let params = UfmcParams::reference();
        let sent = points(params.points() * 4);
        let mut wave = Vec::new();
        UfmcMod::new(params).modulate(&sent, &mut wave);
        assert_eq!(wave.len(), 4 * params.samples());
        let mut got = Vec::new();
        UfmcDemod::new(params).demodulate(&wave, &mut got);
        assert_eq!(got.len(), sent.len());
        for (k, (a, b)) in got.iter().zip(&sent).enumerate() {
            assert!((a - b).norm() < 1e-3, "point {k}: {a} vs {b}");
        }
    }

    #[test]
    fn the_per_bin_division_costs_almost_nothing() {
        let demod = UfmcDemod::new(UfmcParams::reference());
        let worst = demod
            .amplification()
            .iter()
            .fold(0.0f32, |acc, &v| acc.max(v));
        let worst_db = f64::from(worst).power_to_db();
        assert!(worst_db < 1.5, "worst noise amplification {worst_db} dB");
    }

    #[test]
    fn the_subband_filter_buys_out_of_band_suppression() {
        let params = UfmcParams::reference();
        let sent = points(params.points() * 8);
        let mut wave = Vec::new();
        UfmcMod::new(params).modulate(&sent, &mut wave);

        let level = |x: &[Complex<f32>], bin: f64| {
            let n = x.len() as f64;
            x.iter()
                .enumerate()
                .map(|(i, &v)| {
                    widen(v)
                        * Complex::from_polar(
                            1.0,
                            -std::f64::consts::TAU * bin * i as f64 / params.fft as f64,
                        )
                })
                .sum::<Complex<f64>>()
                .norm_sqr()
                / n
        };
        let in_band = level(&wave, 12.0);
        let out_of_band = level(&wave, 40.0);
        let suppression = (in_band / out_of_band).power_to_db();
        assert!(
            suppression > 25.0,
            "out-of-band suppression {suppression} dB"
        );
    }

    #[test]
    fn the_overhead_is_the_filter_tail() {
        let params = UfmcParams::reference();
        assert_eq!(params.samples(), 160);
        let want = (160.0f64 / 128.0).power_to_db();
        assert!((params.overhead_db() - want).abs() < 1e-12);
        assert!((params.overhead_db() - 0.9691).abs() < 1e-4);
    }

    #[test]
    #[should_panic(expected = "a UFMC symbol runs")]
    fn a_prototype_the_receiver_cannot_hold_is_rejected() {
        let mut params = UfmcParams::reference();
        params.filter_len = params.fft + 2;
        let _ = UfmcMod::new(params);
    }

    #[test]
    #[should_panic(expected = "even subband count")]
    fn an_odd_subband_count_is_rejected() {
        let mut params = UfmcParams::reference();
        params.subbands = 3;
        let _ = UfmcMod::new(params);
    }
}

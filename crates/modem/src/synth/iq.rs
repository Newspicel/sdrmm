use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::FracResampler;

/// Marsaglia's xorshift32 shift triple.
const XORSHIFT32: [u32; 3] = [13, 17, 5];

/// Moves a block `freq_hz` up the band.
pub fn shift(iq: &mut [Complex<f32>], freq_hz: f64, rate: f64) {
    for (s, rotor) in iq.iter_mut().zip(carrier(freq_hz, rate)) {
        *s *= rotor;
    }
}

/// Adds uniform noise of peak `amp` from a xorshift32 stream, so a seed always gives the same block.
pub fn add_noise(iq: &mut [Complex<f32>], seed: u32, amp: f32) {
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state << XORSHIFT32[0];
        state ^= state >> XORSHIFT32[1];
        state ^= state << XORSHIFT32[2];
        (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * amp
    };
    for s in iq {
        *s += Complex::new(next(), next());
    }
}

pub fn scale(iq: &mut [Complex<f32>], gain: f32) {
    for s in iq {
        *s *= gain;
    }
}

#[must_use]
pub fn silence(len: usize) -> Vec<Complex<f32>> {
    vec![Complex::new(0.0, 0.0); len]
}

#[must_use]
pub fn resample(iq: &[Complex<f32>], from: f64, to: f64) -> Vec<Complex<f32>> {
    let ratio = to / from;
    let mut resampler = FracResampler::new(ratio);
    let mut out = Vec::with_capacity((iq.len() as f64 * ratio) as usize + 1);
    resampler.process(iq, &mut out);
    out
}

/// Unit phasors of a carrier at `freq_hz`, the phase wrapped every sample so a long run keeps
/// its step.
pub(super) fn carrier(freq_hz: f64, rate: f64) -> impl Iterator<Item = Complex<f32>> {
    let step = TAU * freq_hz / rate;
    (0u64..).map(move |k| Complex::from_polar(1.0, (step * k as f64).rem_euclid(TAU) as f32))
}

#[cfg(test)]
mod tests {
    use sdrmm_dsp::math::complex::mean_power;

    use super::*;
    use crate::synth::ook;

    #[test]
    fn shift_moves_a_dc_carrier_to_the_requested_offset() {
        let mut iq = vec![Complex::new(1.0f32, 0.0); 4_800];
        shift(&mut iq, 1_000.0, 48_000.0);
        let turn = iq[48] / iq[0];
        assert!((turn.re - 1.0).abs() < 1e-3, "{turn}");
        assert!(turn.im.abs() < 1e-3, "{turn}");
    }

    #[test]
    fn resample_stretches_the_block_by_the_rate_ratio() {
        let iq = ook(&vec![1.0f32; 4_800], 1_000.0, 48_000.0);
        let out = resample(&iq, 48_000.0, 96_000.0);
        assert!(out.len().abs_diff(2 * iq.len()) < 64, "{}", out.len());
    }

    #[test]
    fn noise_and_scale_move_the_power_the_way_they_are_named() {
        let mut quiet = silence(4_096);
        add_noise(&mut quiet, 0x5DEE_CE66, 0.1);
        let mut loud = silence(4_096);
        add_noise(&mut loud, 0x5DEE_CE66, 1.0);
        assert!(mean_power(&loud) > 50.0 * mean_power(&quiet));
        let mut halved = loud.clone();
        scale(&mut halved, 0.5);
        assert!((mean_power(&halved) / mean_power(&loud) - 0.25).abs() < 1e-6);
    }
}

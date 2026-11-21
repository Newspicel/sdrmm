mod kaiser;
mod polyphase;
mod pulse;
mod remez;
mod window_sinc;

#[cfg(test)]
use std::f64::consts::PI;

pub use kaiser::{
    design_halfband, design_lowpass_kaiser, halfband_taps, kaiser_beta, kaiser_taps,
    kaiser_transition,
};
pub(crate) use polyphase::reversed_branch;
pub use pulse::{
    design_cosine_biphase, design_cosine_shaping, design_gaussian, design_rrc,
    design_rrc_unit_energy, rrc_pulse,
};
pub use remez::{
    RemezError, design_equiripple, design_halfband_equiripple, design_lowpass_equiripple,
};
pub(crate) use window_sinc::blackman_taps;
pub use window_sinc::{design_bandpass, design_lowpass, design_lowpass_blackman_harris};

#[cfg(test)]
use crate::math::db::Decibels;

pub const MIN_TAPS: usize = 3;

fn unity_dc_gain(h: Vec<f64>) -> Vec<f32> {
    let sum: f64 = h.iter().sum();
    h.into_iter().map(|v| (v / sum) as f32).collect()
}

/// Magnitude response of `h` at `freq` cycles per sample, in dB.
#[cfg(test)]
fn response_db(h: &[f32], freq: f64) -> f64 {
    let (re, im) = h
        .iter()
        .enumerate()
        .fold((0.0, 0.0), |(re, im), (k, &tap)| {
            let phase = -2.0 * PI * freq * k as f64;
            (
                re + f64::from(tap) * phase.cos(),
                im + f64::from(tap) * phase.sin(),
            )
        });
    (re * re + im * im).power_to_db()
}

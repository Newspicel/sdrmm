mod design;
mod kernel;
mod line;
#[cfg(target_arch = "aarch64")]
mod neon;
mod plan;
#[cfg(test)]
mod ragged;
mod run;
#[cfg(any(test, not(any(target_arch = "aarch64", target_arch = "x86_64"))))]
mod scalar;
mod stream;
#[cfg(target_arch = "x86_64")]
mod x86;

pub use design::{
    MIN_TAPS, design_bandpass, design_cosine_biphase, design_cosine_shaping, design_gaussian,
    design_halfband, design_lowpass, design_lowpass_blackman_harris, design_lowpass_kaiser,
    design_rrc, design_rrc_unit_energy, halfband_taps, kaiser_beta, kaiser_taps, kaiser_transition,
    rrc_pulse,
};
pub(crate) use design::{blackman_taps, reversed_branch};
pub(crate) use kernel::{Accumulate, Isa, plane_dot, plane_interpolated};
pub(crate) use line::DelayLine;
#[cfg(test)]
pub(crate) use ragged::in_ragged_blocks;
pub(crate) use stream::StreamFir;

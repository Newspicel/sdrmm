use std::{f64::consts::PI, fmt};

use super::params::CpmParams;
use crate::pulse::phase_pulse;

pub(super) const QUARTER_TURN: f64 = PI / 2.0;

const HALF_INDEX: f64 = 0.5;

const H_TOLERANCE: f64 = 1e-3;

const SPS_TOLERANCE: f64 = 1e-9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LaurentError {
    NotBinary(usize),
    NotHalfIndex(f64),
    FractionalSps(f64),
}

impl fmt::Display for LaurentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotBinary(m) => write!(f, "coherent detection needs 2 levels, got {m}"),
            Self::NotHalfIndex(h) => write!(f, "coherent detection needs h = 0.5, got {h}"),
            Self::FractionalSps(sps) => {
                write!(
                    f,
                    "coherent detection needs whole samples per symbol, got {sps}"
                )
            }
        }
    }
}

impl std::error::Error for LaurentError {}

pub fn laurent_main_pulse(params: &CpmParams) -> Result<Vec<f32>, LaurentError> {
    let sps = whole_sps(params)?;
    let q = phase_pulse(params.freq_pulse());
    let memory = q.len().div_ceil(sps).max(1);
    let span = memory * sps;
    let h = params.h();
    let q_at = |n: isize| -> f64 {
        if n < 0 {
            0.0
        } else {
            q.get(n as usize).map_or(0.5, |&v| f64::from(v))
        }
    };
    let s0 = |n: isize| -> f64 {
        let ramp = if n < span as isize {
            2.0 * PI * h * q_at(n)
        } else {
            PI * h - 2.0 * PI * h * q_at(n - span as isize)
        };
        ramp.sin() / (PI * h).sin()
    };
    let len = (memory + 1) * sps;
    let raw: Vec<f64> = (0..len as isize)
        .map(|n| {
            (0..memory)
                .map(|i| s0(n + (i * sps) as isize))
                .product::<f64>()
        })
        .collect();
    let energy: f64 = raw.iter().map(|v| v * v).sum();
    let scale = energy.sqrt().recip();
    Ok(raw.iter().map(|v| (v * scale) as f32).collect())
}

pub(super) fn whole_sps(params: &CpmParams) -> Result<usize, LaurentError> {
    if params.mapping().m() != 2 {
        return Err(LaurentError::NotBinary(params.mapping().m()));
    }
    if (params.h() - HALF_INDEX).abs() > H_TOLERANCE {
        return Err(LaurentError::NotHalfIndex(params.h()));
    }
    let sps = params.sps();
    let whole = sps.round();
    if (sps - whole).abs() > SPS_TOLERANCE {
        return Err(LaurentError::FractionalSps(sps));
    }
    Ok(whole as usize)
}

pub(super) fn polarity(params: &CpmParams) -> f32 {
    let levels = params.mapping().levels();
    if levels[1] > levels[0] { 1.0 } else { -1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cpm::Mapping,
        pulse::{self, Norm},
    };

    const SPS: f64 = 8.0;

    fn msk() -> CpmParams {
        CpmParams::from_h(Mapping::natural(2), 0.5, pulse::rect(SPS, Norm::Area), SPS)
    }

    #[test]
    fn the_msk_main_pulse_is_a_half_sine_over_two_symbols() {
        let c0 = laurent_main_pulse(&msk()).unwrap();
        assert_eq!(c0.len(), 2 * SPS as usize);
        let peak = c0.iter().copied().fold(0.0f32, f32::max);
        let middle = c0[SPS as usize - 1].max(c0[SPS as usize]);
        assert!((middle - peak).abs() < 1e-6, "peak {peak} sits off centre");
        let energy: f32 = c0.iter().map(|v| v * v).sum();
        assert!((energy - 1.0).abs() < 1e-5);
    }

    #[test]
    fn only_binary_half_index_cpm_is_accepted() {
        let four = CpmParams::from_h(Mapping::natural(4), 0.5, pulse::rect(SPS, Norm::Area), SPS);
        assert_eq!(laurent_main_pulse(&four), Err(LaurentError::NotBinary(4)));
        let wide = CpmParams::from_h(Mapping::natural(2), 1.0, pulse::rect(SPS, Norm::Area), SPS);
        assert_eq!(
            laurent_main_pulse(&wide),
            Err(LaurentError::NotHalfIndex(1.0))
        );
        let fractional =
            CpmParams::from_h(Mapping::natural(2), 0.5, pulse::rect(8.5, Norm::Area), 8.5);
        assert_eq!(
            laurent_main_pulse(&fractional),
            Err(LaurentError::FractionalSps(8.5))
        );
    }
}

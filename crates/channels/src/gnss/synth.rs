use sdrmm_modem::synth::{Render, Signal};
use std::f32::consts::TAU;

use num_complex::Complex;

use crate::gnss::sampled_code;

pub const RATE: f64 = 2_048_000.0;

#[must_use]
pub fn acquisition(
    prn: u8,
    doppler_hz: f32,
    code_phase_samples: usize,
    milliseconds: usize,
) -> Vec<Complex<f32>> {
    let code = sampled_code(prn);
    let len = milliseconds * code.len();
    let mut out = Vec::with_capacity(len);
    for n in 0..len {
        let carrier = Complex::from_polar(1.0, TAU * doppler_hz * n as f32 / RATE as f32);
        out.push(carrier * code[(n + code_phase_samples) % code.len()]);
    }
    out
}

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "gnss",
    label: "GPS L1 C/A",
    rate_hz: 2_048_000.0,
    render: Render::Always(gnss),
}];

fn gnss() -> Vec<Complex<f32>> {
    acquisition(7, 1_000.0, 317, 1_000)
}

/// The reference captures the fixtures are written from.
pub(crate) static RECIPES: &[sdrmm_modem::synth::Recipe] = &[sdrmm_modem::synth::Recipe {
    stem: "gps_l1_ca_prn7_2m048",
    rate: RATE,
    note: "gnss channel -> GPS L1 C/A PRN 7, +1000 Hz Doppler, code phase 158.3 chips",
    iq: || Ok(acquisition(7, 1_000.0, 317, 2)),
}];

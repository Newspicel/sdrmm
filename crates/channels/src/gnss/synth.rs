use sdrmm_modem::synth::{Render, Signal};
use std::f32::consts::TAU;

use num_complex::Complex;

use super::code::{CHIPS, ca_code, sampled_code};
use super::track::{CHIP_RATE, L1_HZ};

pub const RATE: f64 = 2_048_000.0;

#[must_use]
pub fn acquisition(
    prn: u8,
    doppler_hz: f32,
    code_phase_samples: usize,
    milliseconds: usize,
) -> Vec<Complex<f32>> {
    let code = sampled_code(prn, RATE as usize / 1_000);
    let len = milliseconds * code.len();
    let mut out = Vec::with_capacity(len);
    for n in 0..len {
        let carrier = Complex::from_polar(1.0, TAU * doppler_hz * n as f32 / RATE as f32);
        out.push(carrier * code[(n + code_phase_samples) % code.len()]);
    }
    out
}

/// A satellite as it arrives off air: the code rate follows the Doppler, chip 0 starts at
/// sample `code_start`, and the carrier holds no data.
#[must_use]
pub fn satellite(
    prn: u8,
    doppler_hz: f64,
    code_start: f64,
    milliseconds: usize,
) -> Vec<Complex<f32>> {
    let code = ca_code(prn);
    let chip_rate = CHIP_RATE * (1.0 + doppler_hz / L1_HZ);
    (0..milliseconds * RATE as usize / 1_000)
        .map(|n| {
            let t = (n as f64 - code_start) / RATE;
            let chip = (t * chip_rate).rem_euclid(CHIPS as f64) as usize;
            let phase = std::f64::consts::TAU * doppler_hz * n as f64 / RATE + 0.7;
            Complex::new(phase.cos() as f32, phase.sin() as f32) * code[chip]
        })
        .collect()
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

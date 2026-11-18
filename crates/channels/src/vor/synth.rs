use sdrmm_modem::synth::{AUDIO_HZ, Render, Signal};
use std::f64::consts::TAU;

use num_complex::Complex;

pub const RATE: f64 = 48_000.0;

#[must_use]
pub fn transmission(radial_deg: f64, seconds: usize) -> Vec<Complex<f32>> {
    (0..RATE as usize * seconds)
        .map(|index| {
            let time = index as f64 / RATE;
            let reference = TAU * 30.0 * time;
            let variable = reference - radial_deg.to_radians();
            let subcarrier = TAU * 9_960.0 * time + 16.0 * reference.sin();
            let envelope = 1.0 + 0.3 * variable.cos() + 0.3 * subcarrier.cos();
            Complex::new(envelope as f32, 0.0)
        })
        .collect()
}

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "vor",
    label: "VOR",
    rate_hz: AUDIO_HZ,
    render: Render::Always(vor),
}];

fn vor() -> Vec<Complex<f32>> {
    transmission(123.0, 2)
}

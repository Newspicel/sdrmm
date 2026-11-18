use num_complex::Complex;
use sdrmm_modem::synth::{Render, Signal};

use sdrmm_modem::synth::{add_noise, silence};

#[must_use]
pub fn noise(seconds: f64, amplitude: f32, rate: f64, seed: u32) -> Vec<Complex<f32>> {
    let mut iq = silence((seconds * rate) as usize);
    add_noise(&mut iq, seed, amplitude);
    iq
}

#[must_use]
pub fn switched(y_db: f32, seconds_each: f64, rate: f64) -> Vec<Complex<f32>> {
    let mut iq = noise(seconds_each, 0.1, rate, 0x0C01D);
    iq.extend(noise(
        seconds_each,
        0.1 * 10f32.powf(y_db / 20.0),
        rate,
        0x0407,
    ));
    iq
}

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "noise_source",
    label: "Noise source · switched",
    rate_hz: NOISE_SOURCE_RATE,
    render: Render::Always(noise_source),
}];

const NOISE_SOURCE_RATE: f64 = 500_000.0;

fn noise_source() -> Vec<Complex<f32>> {
    switched(10.0, 1.0, NOISE_SOURCE_RATE)
}

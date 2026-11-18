//! Measures of real audio for tests: one tone's level, and the strongest tone.

use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::math::fft::Transform;

/// The amplitude of the `freq_hz` component of `audio`.
#[must_use]
pub fn tone_amplitude(audio: &[f32], freq_hz: f64, rate: f64) -> f32 {
    let step = TAU * freq_hz / rate;
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (n, &s) in audio.iter().enumerate() {
        re += f64::from(s) * (step * n as f64).cos();
        im += f64::from(s) * (step * n as f64).sin();
    }
    (2.0 * re.hypot(im) / audio.len() as f64) as f32
}

fn half_spectrum(audio: &[f32]) -> Vec<f64> {
    let n = audio.len();
    let mut buf: Vec<Complex<f32>> = audio.iter().map(|&v| Complex::new(v, 0.0)).collect();
    Transform::forward(n).process(&mut buf);
    buf[..=n / 2]
        .iter()
        .map(|v| f64::from(v.norm_sqr()))
        .collect()
}

fn bin_power(power: &[f64], bin: usize) -> f64 {
    let lo = bin.saturating_sub(3);
    let hi = (bin + 3).min(power.len() - 1);
    power[lo..=hi].iter().sum()
}

/// The strongest tone in `audio` and the share of the energy it holds against the rest.
#[must_use]
pub fn dominant_tone(audio: &[f32], rate: f64) -> (f64, f64) {
    let power = half_spectrum(audio);
    let peak = power
        .iter()
        .enumerate()
        .skip(1)
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map_or(0, |(i, _)| i);
    let signal = bin_power(&power, peak);
    let rest = (power.iter().sum::<f64>() - signal).max(1e-30);
    (peak as f64 * rate / audio.len() as f64, signal / rest)
}

/// The share of the energy in `audio` that sits at `freq_hz`.
#[must_use]
pub fn tone_power(audio: &[f32], freq_hz: f64, rate: f64) -> f64 {
    let power = half_spectrum(audio);
    let bin = (freq_hz * audio.len() as f64 / rate).round() as usize;
    bin_power(&power, bin) / power.iter().sum::<f64>().max(1e-30)
}

pub(super) const RATE: f64 = 48_000.0;

/// Amplitude of the `freq_hz` component of `x`, by correlating against it.
pub(super) fn tone_amplitude(x: &[f32], freq_hz: f64) -> f32 {
    let w = std::f64::consts::TAU * freq_hz / RATE;
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (n, &s) in x.iter().enumerate() {
        re += f64::from(s) * (w * n as f64).cos();
        im += f64::from(s) * (w * n as f64).sin();
    }
    (2.0 * re.hypot(im) / x.len() as f64) as f32
}

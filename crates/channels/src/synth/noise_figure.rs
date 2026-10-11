use num_complex::Complex;

use super::{add_noise, silence};

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

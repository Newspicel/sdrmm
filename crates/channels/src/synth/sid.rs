use num_complex::Complex;

use super::{add_noise, ook, silence};

#[must_use]
pub fn carriers(
    carriers: &[(f64, f32)],
    centre_hz: f64,
    seconds: f64,
    rate: f64,
    seed: u32,
) -> Vec<Complex<f32>> {
    let len = (seconds * rate) as usize;
    let mut iq = silence(len);
    for &(frequency_hz, amplitude) in carriers {
        let tone = ook(&vec![amplitude; len], frequency_hz - centre_hz, rate);
        iq.iter_mut().zip(tone).for_each(|(s, t)| *s += t);
    }
    add_noise(&mut iq, seed, 0.01);
    iq
}

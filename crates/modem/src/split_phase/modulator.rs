use std::f64::consts::TAU;

use num_complex::Complex;

/// Phase-modulates `bits` as split-phase chips, a one being high then low, onto a carrier
/// `offset_hz` from the centre with peak deviation `index_rad`. Each sample takes the mean
/// chip level over its own interval, so a chip edge between samples lands between levels as
/// it would after a receiver's filter.
#[must_use]
pub fn modulate(
    bits: &[bool],
    sample_rate: f64,
    bit_rate: f64,
    index_rad: f64,
    offset_hz: f64,
) -> Vec<Complex<f32>> {
    let chips_per_sample = 2.0 * bit_rate / sample_rate;
    let chips = 2 * bits.len();
    let samples = (chips as f64 / chips_per_sample).floor() as usize;
    let level = |chip: usize| {
        let bit = bits[(chip / 2).min(bits.len() - 1)];
        if bit == chip.is_multiple_of(2) {
            1.0
        } else {
            -1.0
        }
    };
    (0..samples)
        .map(|n| {
            let start = n as f64 * chips_per_sample;
            let end = (start + chips_per_sample).min(chips as f64);
            let mut mean = 0.0;
            let mut at = start;
            while at < end {
                let chip = at.floor();
                let next = (chip + 1.0).min(end);
                mean += level(chip as usize) * (next - at);
                at = next;
            }
            mean /= chips_per_sample;
            let phase = TAU * offset_hz * n as f64 / sample_rate + index_rad * mean;
            let (sin, cos) = phase.sin_cos();
            Complex::new(cos as f32, sin as f32)
        })
        .collect()
}

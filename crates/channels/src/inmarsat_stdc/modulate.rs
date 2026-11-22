use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::filter::fir::rrc_pulse;

use super::demod::RRC_BETA;

const PULSE_SPAN_SYMBOLS: f64 = 8.0;

/// Root-raised-cosine BPSK, as the NCS sends it, each symbol carrying the area of a full-length
/// NRZ pulse so the power matches a unit-level rectangle.
pub fn modulate(
    symbols: &[u8],
    symbol_rate: f64,
    sample_rate: f64,
    freq_offset_hz: f64,
    amplitude: f32,
) -> Vec<Complex<f32>> {
    let samples_per_symbol = sample_rate / symbol_rate;
    let span = (PULSE_SPAN_SYMBOLS * samples_per_symbol).ceil() as i64;
    let energy = (-span..=span)
        .map(|n| rrc_pulse(n as f64 / samples_per_symbol, RRC_BETA).powi(2))
        .sum::<f64>()
        .sqrt();
    let total = (symbols.len() as f64 * samples_per_symbol).round() as usize + span as usize;
    let mut baseband = vec![0.0f64; total];
    for (index, &symbol) in symbols.iter().enumerate() {
        let level = if symbol == 1 { 1.0 } else { -1.0 } * samples_per_symbol / energy;
        let center = (index as f64 + 0.5) * samples_per_symbol + span as f64;
        let first = (center as i64 - span).max(0);
        let last = (center as i64 + span).min(total as i64 - 1);
        for n in first..=last {
            baseband[n as usize] +=
                level * rrc_pulse((n as f64 - center) / samples_per_symbol, RRC_BETA);
        }
    }
    baseband
        .into_iter()
        .enumerate()
        .map(|(n, level)| {
            let phase = TAU * freq_offset_hz * n as f64 / sample_rate;
            Complex::from_polar(amplitude * level as f32, phase as f32)
        })
        .collect()
}

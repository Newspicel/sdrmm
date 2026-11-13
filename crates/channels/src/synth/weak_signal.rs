use num_complex::Complex;

use super::{Need, SynthError};
use crate::weak_signal::{FT4, FT8, ftx_waveform, pack_ftx, wspr_waveform};

const RATE: usize = 12_000;
const AMPLITUDE: f32 = 0.6;

fn slot(audio: &[f32], slot_samples: usize, start_samples: usize) -> Vec<Complex<f32>> {
    let mut iq = vec![Complex::new(0.0, 0.0); slot_samples];
    for (target, &sample) in iq[start_samples..].iter_mut().zip(audio) {
        target.re = AMPLITUDE * sample;
    }
    iq
}

pub fn ft8_slot(call: &str, grid: &str, audio_hz: f32) -> Result<Vec<Complex<f32>>, SynthError> {
    let payload = pack_ftx(&format!("CQ {call} {grid}")).need("FT8 message")?;
    Ok(slot(
        &ftx_waveform(&FT8, payload, f64::from(audio_hz)),
        15 * RATE,
        RATE / 2,
    ))
}

pub fn ft4_slot(call: &str, grid: &str, audio_hz: f32) -> Result<Vec<Complex<f32>>, SynthError> {
    let payload = pack_ftx(&format!("CQ {call} {grid}")).need("FT4 message")?;
    Ok(slot(
        &ftx_waveform(&FT4, payload, f64::from(audio_hz)),
        15 * RATE / 2,
        RATE / 2,
    ))
}

pub fn wspr_slot(
    call: &str,
    grid: &str,
    power_dbm: i32,
    audio_hz: f32,
) -> Result<Vec<Complex<f32>>, SynthError> {
    let audio =
        wspr_waveform(&format!("{call} {grid} {power_dbm}"), audio_hz).need("WSPR message")?;
    Ok(slot(&audio, 120 * RATE, RATE))
}

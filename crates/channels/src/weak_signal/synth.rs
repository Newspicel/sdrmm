use num_complex::Complex;
use sdrmm_modem::synth::{AUDIO_HZ, Render, Rendered, Signal, resample};

use crate::weak_signal::{FT4, FT8, ftx_waveform, pack_ftx, wspr_waveform};
use sdrmm_modem::synth::{Need, SynthError};

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

pub(crate) static FT8_SIGNALS: &[Signal] = &[Signal {
    id: "ft8",
    label: "FT8",
    rate_hz: AUDIO_HZ,
    render: Render::Fallible(ft8),
}];

pub(crate) static FT4_SIGNALS: &[Signal] = &[Signal {
    id: "ft4",
    label: "FT4",
    rate_hz: AUDIO_HZ,
    render: Render::Fallible(ft4),
}];

pub(crate) static WSPR_SIGNALS: &[Signal] = &[Signal {
    id: "wspr",
    label: "WSPR",
    rate_hz: AUDIO_HZ,
    render: Render::Fallible(wspr),
}];

fn ft8() -> Rendered {
    Ok(resample(
        &ft8_slot("W1AW", "FN42", 1_500.0)?,
        12_000.0,
        AUDIO_HZ,
    ))
}

fn ft4() -> Rendered {
    Ok(resample(
        &ft4_slot("W1AW", "FN42", 1_500.0)?,
        12_000.0,
        AUDIO_HZ,
    ))
}

fn wspr() -> Rendered {
    Ok(resample(
        &wspr_slot("W1AW", "FN42", 33, 1_500.0)?,
        12_000.0,
        AUDIO_HZ,
    ))
}

use num_complex::Complex;
use sdrmm_modem::synth::{AUDIO_HZ, Render, Rendered, Signal, resample};

use crate::weak_signal::{
    FT4, FT8, Fst4Params, Fst4wParams, Q65Params, fst4_waveform, ftx_waveform, msk144_waveform,
    pack_ftx, q65_waveform, wspr_waveform,
};
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

/// A 1.4 s MSK144 ping of `text` in 3 s of silence.
pub fn msk144_ping(text: &str, audio_hz: f32) -> Result<Vec<Complex<f32>>, SynthError> {
    let payload = pack_ftx(text).need("MSK144 message")?;
    let audio = msk144_waveform(payload, f64::from(audio_hz), 20).need("MSK144 message")?;
    Ok(slot(&audio, 3 * RATE, RATE / 2))
}

/// A Q65-60A period sending `CQ call grid`.
pub fn q65_slot(call: &str, grid: &str, audio_hz: f32) -> Result<Vec<Complex<f32>>, SynthError> {
    let payload = pack_ftx(&format!("CQ {call} {grid}")).need("Q65 message")?;
    let setup = Q65Params::default().setup();
    Ok(slot(
        &q65_waveform(setup, payload, f64::from(audio_hz)),
        setup.period_s as usize * RATE,
        RATE,
    ))
}

pub(crate) static Q65_SIGNALS: &[Signal] = &[Signal {
    id: "q65",
    label: "Q65",
    rate_hz: AUDIO_HZ,
    render: Render::Fallible(q65),
}];

fn q65() -> Rendered {
    Ok(resample(
        &q65_slot("W1AW", "FN42", 1_500.0)?,
        12_000.0,
        AUDIO_HZ,
    ))
}

/// An FST4-60 period sending `CQ call grid`.
pub fn fst4_slot(call: &str, grid: &str, audio_hz: f32) -> Result<Vec<Complex<f32>>, SynthError> {
    let setup = Fst4Params::default().setup();
    let audio = fst4_waveform(setup, &format!("CQ {call} {grid}"), f64::from(audio_hz))
        .need("FST4 message")?;
    Ok(slot(&audio, setup.period_s as usize * RATE, RATE))
}

/// An FST4W-120 period sending the beacon `call grid dbm`.
pub fn fst4w_slot(
    call: &str,
    grid: &str,
    dbm: i32,
    audio_hz: f32,
) -> Result<Vec<Complex<f32>>, SynthError> {
    let setup = Fst4wParams::default().setup();
    let audio = fst4_waveform(setup, &format!("{call} {grid} {dbm}"), f64::from(audio_hz))
        .need("FST4W beacon")?;
    Ok(slot(&audio, setup.period_s as usize * RATE, RATE))
}

pub(crate) static FST4_SIGNALS: &[Signal] = &[Signal {
    id: "fst4",
    label: "FST4",
    rate_hz: AUDIO_HZ,
    render: Render::Fallible(fst4),
}];

fn fst4() -> Rendered {
    Ok(resample(
        &fst4_slot("W1AW", "FN42", 1_500.0)?,
        12_000.0,
        AUDIO_HZ,
    ))
}

pub(crate) static MSK144_SIGNALS: &[Signal] = &[Signal {
    id: "msk144",
    label: "MSK144",
    rate_hz: AUDIO_HZ,
    render: Render::Fallible(msk144),
}];

fn msk144() -> Rendered {
    Ok(resample(
        &msk144_ping("CQ W1AW FN42", 1_500.0)?,
        12_000.0,
        AUDIO_HZ,
    ))
}

pub fn jt65_slot(call: &str, grid: &str, audio_hz: f32) -> Result<Vec<Complex<f32>>, SynthError> {
    let audio =
        super::jt65::waveform(&format!("CQ {call} {grid}"), audio_hz).need("JT65 message")?;
    Ok(slot(&audio, 60 * RATE, RATE))
}

pub fn jt9_slot(call: &str, grid: &str, audio_hz: f32) -> Result<Vec<Complex<f32>>, SynthError> {
    let audio = super::jt9::waveform(&format!("CQ {call} {grid}"), audio_hz).need("JT9 message")?;
    Ok(slot(&audio, 60 * RATE, RATE))
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

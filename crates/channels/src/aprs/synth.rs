use sdrmm_decoder::encoder::{NativeModulator, TxPayload, play};
use sdrmm_modem::synth::{NARROW_HZ, Need, Render, Rendered, Signal, resample};

use super::{AprsEncoderSettings, AprsModulator};

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[
    Signal {
        id: "aprs",
        label: "APRS · AFSK1200",
        rate_hz: NARROW_HZ,
        render: Render::Fallible(aprs),
    },
    Signal {
        id: "aprs_weather",
        label: "APRS weather",
        rate_hz: NARROW_HZ,
        render: Render::Fallible(aprs_weather),
    },
];

fn aprs() -> Rendered {
    packet("SDRmm signal generator")
}

fn aprs_weather() -> Rendered {
    packet("!4903.50N/07201.75W_220/004g005t077r000p000P000h50b09900SDRmm")
}

fn packet(text: &str) -> Rendered {
    let mut tx = AprsModulator::new(&AprsEncoderSettings::default()).need("APRS modulator")?;
    let iq = play(&mut tx, TxPayload::Frame(frame(text))).need("APRS packet")?;
    Ok(resample(&iq, AprsModulator::rate_hz(), NARROW_HZ))
}

fn frame(text: &str) -> Vec<u8> {
    let mut frame = Vec::new();
    for call in ["APRS  ", "DL0SDR"] {
        frame.extend(call.bytes().map(|byte| byte << 1));
        frame.push(0x60);
    }
    let last = frame.len() - 1;
    frame[last] |= 1;
    frame.extend([0x03, 0xF0]);
    frame.extend(text.bytes());
    frame
}

/// The reference captures the fixtures are written from.
pub(crate) static RECIPES: &[sdrmm_modem::synth::Recipe] = &[sdrmm_modem::synth::Recipe {
    stem: "aprs_afsk1200_240k",
    rate: sdrmm_modem::synth::NARROW_HZ,
    note: "aprs channel at -40 kHz -> DL1ABC-9>APRS,WIDE1-1 at 52.5, 13.4",
    iq: fixture_packet,
}];

fn fixture_packet() -> sdrmm_modem::synth::Rendered {
    use crate::aprs::{AprsEncoderSettings, AprsMode, AprsModulator};
    use sdrmm_decoder::encoder::{Modulator, NativeModulator, TxPayload};
    use sdrmm_modem::synth::Need;
    let mut tx = AprsModulator::new(&AprsEncoderSettings {
        mode: AprsMode::Afsk1200,
    })
    .need("APRS modulator")?;
    tx.submit(TxPayload::Frame(AprsModulator::ui_frame(
        "DL1ABC-9",
        "APRS",
        &["WIDE1-1"],
        "!5230.00N/01324.00E>SDRmm fixture",
    )))
    .need("APRS UI frame")?;
    let iq = sdrmm_modem::synth::resample(
        &sdrmm_decoder::encoder::burst(&mut tx),
        sdrmm_modem::synth::AUDIO_HZ,
        sdrmm_modem::synth::NARROW_HZ,
    );
    Ok(sdrmm_modem::synth::shifted(
        iq,
        -40_000.0,
        sdrmm_modem::synth::NARROW_HZ,
    ))
}

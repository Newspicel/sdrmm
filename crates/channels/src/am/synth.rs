use sdrmm_decoder::encoder::{NativeModulator, TxPayload, play};
use sdrmm_modem::synth::{
    NARROW_HZ, Need, Render, Rendered, Signal, resample, seconds, speech_audio,
};

use super::{AmEncoderSettings, AmModulator};

const AUDIO_RATE_HZ: f64 = sdrmm_decoder::AUDIO_RATE as f64;

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "am_voice",
    label: "AM voice",
    rate_hz: NARROW_HZ,
    render: Render::Fallible(am_voice),
}];

fn am_voice() -> Rendered {
    let mut tx = AmModulator::new(&AmEncoderSettings::default()).need("AM modulator")?;
    let audio = speech_audio(AUDIO_RATE_HZ, seconds(AUDIO_RATE_HZ, 3.0));
    let iq = play(&mut tx, TxPayload::Audio(audio)).need("AM voice")?;
    Ok(resample(&iq, AmModulator::rate_hz(), NARROW_HZ))
}

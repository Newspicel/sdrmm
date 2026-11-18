use sdrmm_decoder::encoder::{NativeModulator, TxPayload, play};
use sdrmm_modem::synth::{
    AUDIO_HZ, Need, Render, Rendered, Signal, resample, seconds, speech_audio,
};
use sdrmm_wire::Sideband;

use super::{SsbEncoderSettings, SsbModulator};

const AUDIO_RATE_HZ: f64 = sdrmm_decoder::AUDIO_RATE as f64;

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "ssb_voice",
    label: "SSB voice · USB",
    rate_hz: AUDIO_HZ,
    render: Render::Fallible(ssb_voice),
}];

fn ssb_voice() -> Rendered {
    let mut tx = SsbModulator::new(&SsbEncoderSettings {
        sideband: Sideband::Usb,
        ..SsbEncoderSettings::default()
    })
    .need("SSB modulator")?;
    let audio = speech_audio(AUDIO_RATE_HZ, seconds(AUDIO_RATE_HZ, 3.0));
    let iq = play(&mut tx, TxPayload::Audio(audio)).need("SSB voice")?;
    Ok(resample(&iq, SsbModulator::rate_hz(), AUDIO_HZ))
}

use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn nfm_decoder() -> Positive {
    reads(&[r#""ctcss_hz":88.5"#, r#""open":true"#])
        .audio(16_000)
        .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new(
        "nfm",
        &NfmParams {
            tone_mode: NfmToneMode::Detect,
            ..NfmParams::default()
        },
    )
    .expect("nfm settings");
    let rate = Fixture::rate(&params);
    let len = (rate * 3.0) as usize;
    let audio = synth::mix(
        &synth::ctcss_audio(88.5, 0.15, rate, len),
        &synth::speech_audio(rate, len),
    );
    Fixture::new(
        params,
        sdrmm_modem::synth::fm_modulate(&audio, 2_500.0, rate),
    )
}

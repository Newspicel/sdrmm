use super::*;
use crate::builtin::fixtures::{Fixture, Positive, keyed_after_noise, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn ltr_decoder() -> Positive {
    reads(&[
        r#""destination":1284"#,
        r#""destination_call":"0-05-004""#,
        r#""channel":5"#,
        r#""rest_channel":2"#,
    ])
    .audio(24_000)
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = LtrParams::default();
    Fixture::new(
        ChannelParams::new("ltr", &params).expect("ltr settings"),
        keyed_after_noise(synth::transmission(
            synth::call(),
            12,
            deviation_hz(&params),
            INPUT_RATE_HZ,
        )),
    )
}

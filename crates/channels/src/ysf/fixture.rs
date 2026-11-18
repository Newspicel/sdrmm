use super::*;
use crate::builtin::fixtures::{Fixture, Positive, keyed_after_noise, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn ysf_decoder() -> Positive {
    reads(&[
        r#""source_call":"DL1ABC""#,
        r#""destination_call":"ALL""#,
        "\"via\":\"DB0XYZ \u{2192} DB0ABC\"",
        r#""destination":7"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("ysf", &YsfParams::default()).expect("ysf settings"),
        keyed_after_noise(synth::transmission_with_callsigns(
            &synth::Fich::default(),
            &synth::Call::default(),
            48_000.0,
        )),
    )
}

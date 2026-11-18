use super::*;
use crate::builtin::fixtures::{Fixture, Positive, keyed_after_noise, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn m17_decoder() -> Positive {
    reads(&[
        r#""source_call":"DL1ABC""#,
        r#""destination_call":"ALL""#,
        r#""opcode":"stream""#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("m17", &M17Params::default()).expect("m17 settings"),
        keyed_after_noise(synth::transmission("ALL", "DL1ABC", 48_000.0)),
    )
}

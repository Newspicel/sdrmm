use super::*;
use crate::builtin::fixtures::{Fixture, Positive, keyed_after_noise, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn dstar_decoder() -> Positive {
    reads(&[
        r#""source_call":"DL1ABC""#,
        r#""destination_call":"CQCQCQ""#,
        r#""via":"DB0ABC B""#,
        r#""text":"hello from SDRmm""#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("dstar", &DstarParams::default()).expect("dstar settings"),
        keyed_after_noise(synth::transmission(&synth::Call::default(), 48_000.0)),
    )
}

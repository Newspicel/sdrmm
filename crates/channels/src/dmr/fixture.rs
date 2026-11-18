use super::*;
use crate::builtin::fixtures::{Fixture, Positive, keyed_after_noise, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn dmr_decoder() -> Positive {
    reads(&[
        r#""color_code":1"#,
        r#""source":2621001"#,
        r#""destination":505"#,
        r#""crc_verified":true"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("dmr", &DmrParams::default()).expect("dmr settings"),
        keyed_after_noise(synth::transmission(&synth::Call::default(), 48_000.0)),
    )
}

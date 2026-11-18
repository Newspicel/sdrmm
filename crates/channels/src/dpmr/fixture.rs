use super::*;
use crate::builtin::fixtures::{Fixture, Positive, keyed_after_noise, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn dpmr_decoder() -> Positive {
    reads(&[
        r#""color_code":37"#,
        r#""source":1193046"#,
        r#""destination":65535"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("dpmr", &DpmrParams::default()).expect("dpmr settings"),
        keyed_after_noise(synth::transmission(&synth::Call::default(), 48_000.0)),
    )
}

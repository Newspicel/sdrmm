use super::*;
use crate::builtin::fixtures::{Fixture, Positive, keyed_after_noise, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn p25_decoder() -> Positive {
    reads(&[
        r#""color_code":659"#,
        r#""source":11259375"#,
        r#""destination":4609"#,
        r#""message_indicator":"0123456789abcdef12""#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("p25", &P25Params::default()).expect("p25 settings"),
        keyed_after_noise(synth::transmission(0x293, 48_000.0)),
    )
}

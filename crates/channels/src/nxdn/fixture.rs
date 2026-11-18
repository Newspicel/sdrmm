use super::*;
use crate::builtin::fixtures::{Fixture, Positive, keyed_after_noise, reads};
use sdrmm_wire::ChannelParams;

const NXDN_DESTINATION: u16 = 2_002;
const NXDN_SOURCE: u16 = 1_001;
const NXDN_RAN: u8 = 1;

pub(crate) fn nxdn_decoder() -> Positive {
    reads(&[
        r#""color_code":1"#,
        r#""source":1001"#,
        r#""destination":2002"#,
        r#""opcode":"voice call""#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("nxdn", &NxdnParams::default()).expect("nxdn settings"),
        keyed_after_noise(synth::addressed_transmission(
            &synth::Shape::default(),
            NXDN_RAN,
            NXDN_SOURCE,
            NXDN_DESTINATION,
            true,
            48_000.0,
        )),
    )
}

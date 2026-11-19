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

pub(crate) fn p25p2_decoder() -> Positive {
    reads(&[
        r#""network_id":781824"#,
        r#""system_id":356"#,
        r#""color_code":356"#,
        r#""opcode":"Motorola patch add""#,
        r#""destination":3202"#,
        r#""text":"TG 201""#,
    ])
    .hearing(|| {
        Fixture::new(
            ChannelParams::of(&P25p2Params::default()).expect("p25p2 settings"),
            sdrmm_decoder_test_support::cf32_le(include_bytes!(
                "../../../../fixtures/p25p2_mmr_idle_48k.sigmf-data"
            )),
        )
        .heard()
    })
}

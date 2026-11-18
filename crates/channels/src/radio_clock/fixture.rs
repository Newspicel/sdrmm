use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_decoder_test_support::cf32_le;
use sdrmm_wire::ChannelParams;

pub(crate) fn radio_clock_decoder() -> Positive {
    reads(&[
        r#""standard":"dcf77""#,
        r#""datetime":"2026-10-03T22:04:00+02:00""#,
        r#""dst":true"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("radio_clock", &RadioClockParams::default())
            .expect("radio_clock settings"),
        cf32_le(include_bytes!(
            "../../../../fixtures/radio_clock_dcf77_offair_2k.sigmf-data"
        )),
    )
}

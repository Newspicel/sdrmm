use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn horus_decoder() -> Positive {
    reads(&[
        r#""raw":"000900071E2A000000000000000000000000259A6B14""#,
        r#""version":1"#,
        r#""payload":"0""#,
        r#""sequence":9"#,
        r#""time":"07:30:42""#,
        r#""temperature_c":37.0"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("horus", &HorusParams::default()).expect("horus settings"),
        sdrmm_decoder_test_support::cf32_le(include_bytes!(
            "../../../../fixtures/horus/horus_v1_iq_8k.sigmf-data"
        )),
    )
}

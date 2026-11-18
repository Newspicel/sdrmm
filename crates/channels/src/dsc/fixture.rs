use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_decoder_test_support::cf32_le;
use sdrmm_wire::ChannelParams;

pub(crate) fn dsc_decoder() -> Positive {
    reads(&[
        r#""station":"002191000""#,
        r#""to":"231700000""#,
        r#""category":"safety""#,
        r#""crc_ok":true"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("dsc", &DscParams::default()).expect("dsc settings"),
        cf32_le(include_bytes!(
            "../../../../fixtures/dsc_offair_8k.sigmf-data"
        )),
    )
}

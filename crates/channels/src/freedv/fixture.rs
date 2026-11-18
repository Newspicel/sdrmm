use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_decoder_test_support::cf32_le;
use sdrmm_wire::ChannelParams;

pub(crate) fn freedv_decoder() -> Positive {
    reads(&[r#""mode":"FreeDV""#, r#""opcode":"1600""#]).hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("freedv", &FreeDvParams::default()).expect("freedv settings"),
        cf32_le(include_bytes!(
            "../../../../fixtures/freedv_1600_8k.sigmf-data"
        )),
    )
}

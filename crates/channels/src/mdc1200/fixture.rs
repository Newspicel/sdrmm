use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_decoder_test_support::cf32_le;
use sdrmm_wire::ChannelParams;

pub(crate) fn mdc1200_decoder() -> Positive {
    reads(&[r#""unit":6007"#, r#""op":1"#, r#""arg":0"#]).hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("mdc1200", &MdcParams::default()).expect("mdc settings"),
        cf32_le(include_bytes!(
            "../../../../fixtures/mdc1200_unit1777_offair_48k.sigmf-data"
        )),
    )
}

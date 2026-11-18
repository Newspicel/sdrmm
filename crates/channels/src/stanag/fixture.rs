use super::*;
use crate::builtin::fixtures::{Fixture, Positive, ci16_le, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn stanag_decoder() -> Positive {
    reads(&[r#""modem":"stanag_4285""#, r#""confidence":1.0"#]).hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("stanag", &StanagParams::default()).expect("stanag settings"),
        ci16_le(include_bytes!(
            "../../../../fixtures/stanag/stanag_4285_9k6.sigmf-data"
        )),
    )
}

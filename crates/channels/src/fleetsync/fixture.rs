use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_decoder_test_support::cf32_le;
use sdrmm_wire::ChannelParams;

pub(crate) fn fleetsync_decoder() -> Positive {
    reads(&[
        r#""version":"two""#,
        r#""from_fleet":107"#,
        r#""from_unit":1772"#,
        r#""command":128"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("fleetsync", &FleetSyncParams::default()).expect("fleetsync settings"),
        cf32_le(include_bytes!(
            "../../../../fixtures/fleetsync2_fleet107_offair_48k.sigmf-data"
        )),
    )
}

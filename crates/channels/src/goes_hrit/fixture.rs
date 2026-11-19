use super::*;
use crate::builtin::fixtures::{Fixture, Positive, ci16_le, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn goes_hrit_decoder() -> Positive {
    reads(&[
        "A_WUCN13CWWG262231_C_KWIN_20250626223159_428276-1-SVRW13CN.lrit",
        "SEVERE THUNDERSTORM WARNING",
        "A_SACA33KWBC262200_C_KWIN_20250626223202_428280-2-SAHOURLY.lrit",
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("goes_hrit", &GoesHritParams::default()).expect("settings");
    let iq = ci16_le(include_bytes!(
        "../../../../fixtures/goes_hrit_offair_1m854.sigmf-data"
    ));
    Fixture::new(params, iq).tuned(DOWNLINK_HZ)
}

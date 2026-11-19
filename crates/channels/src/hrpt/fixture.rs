use super::*;
use crate::builtin::fixtures::{Fixture, Positive, ci16_le, reads, with_tail};
use sdrmm_wire::ChannelParams;

const METEOR_HZ: f64 = 1_700_000_000.0;

pub(crate) fn hrpt_decoder() -> Positive {
    reads(&[
        r#""lines":4"#,
        r#""first_line":"05:22:29 UTC""#,
        r#""picture":"infrared""#,
    ])
    .pictures(1)
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("hrpt", &HrptParams::default()).expect("hrpt settings");
    let rate = Fixture::rate(&params);
    let iq = ci16_le(include_bytes!(
        "../../../../fixtures/hrpt_meteor_offair_2m66.sigmf-data"
    ));
    Fixture::new(params, with_tail(iq, rate, 2.5)).tuned(METEOR_HZ)
}

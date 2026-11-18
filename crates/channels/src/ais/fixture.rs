use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn ais_decoder() -> Positive {
    reads(&[
        r#""mmsi":244010000"#,
        r#""heading_deg":88"#,
        r#""nav_status":0"#,
    ])
    .at(52.3702, 4.8952)
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("ais", &AisParams::default()).expect("ais settings");
    let report = synth::PositionReport {
        mmsi: 244_010_000,
        lat: 52.3702,
        lon: 4.8952,
        sog_kt: 12.4,
        cog_deg: 87.5,
        heading_deg: 88,
        nav_status: 0,
    };
    let rate = Fixture::rate(&params);
    let iq = synth::burst(&synth::position_payload(&report), rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

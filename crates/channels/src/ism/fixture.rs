use super::*;
use crate::builtin::fixtures::{Fixture, Positive, cu8, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn ism_decoder() -> Positive {
    reads(&[
        r#""model":"Nexus-TH""#,
        r#""id":"181""#,
        r#""temperature_c":19.0"#,
        r#""humidity_pct":71"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("ism", &IsmParams::default()).expect("ism settings");
    let rate = Fixture::rate(&params);
    let iq = cu8(include_bytes!(
        "../../../../fixtures/ism/nexus_th_433m92_250k.sigmf-data"
    ));
    Fixture::new(params, with_tail(iq, rate, 0.25))
}

use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn wefax_decoder() -> Positive {
    reads(&[
        r#""ioc":"ioc576""#,
        r#""lpm":"lpm120""#,
        r#""lines":120"#,
        r#""complete":true"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("wefax", &WefaxParams::default()).expect("wefax settings");
    let rate = Fixture::rate(&params);
    let chart = synth::bars(WefaxIoc::Ioc576, 120);
    let iq = synth::transmission(WefaxIoc::Ioc576, WefaxLpm::Lpm120, &chart, rate);
    Fixture::new(params, with_tail(iq, rate, 2.0))
}

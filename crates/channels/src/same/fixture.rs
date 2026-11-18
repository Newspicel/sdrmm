use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn same_decoder() -> Positive {
    reads(&[
        r#""originator":"WXR""#,
        r#""event":"TOR""#,
        r#""station":"KEC55/NW""#,
        r#""valid_minutes":30"#,
        r#""name":"Harris, TX""#,
        r#""copies":2"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("same", &SameParams::default()).expect("same settings");
    let rate = Fixture::rate(&params);
    let iq = synth::transmission("ZCZC-WXR-TOR-048201+0030-1232200-KEC55/NW-", 2, rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

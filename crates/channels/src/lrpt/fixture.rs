use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn lrpt_decoder() -> Positive {
    reads(&[
        r#""mode":"oqpsk72""#,
        r#""apids":[64,65,66]"#,
        r#""complete":true"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let mode = LrptMode::Oqpsk72;
    let params = ChannelParams::new("lrpt", &LrptParams { mode }).expect("lrpt settings");
    let rate = Fixture::rate(&params);
    let iq = synth::transmission(mode, 2, rate);
    Fixture::new(params, with_tail(iq, rate, 2.5))
}

use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn apt_decoder() -> Positive {
    reads(&[
        r#""lines":240"#,
        r#""channel_a":"ch2""#,
        r#""channel_b":"ch4""#,
        r#""complete":true"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("apt", &AptParams::default()).expect("apt settings");
    let rate = Fixture::rate(&params);
    let iq = synth::transmission(240, AvhrrChannel::Ch2, AvhrrChannel::Ch4, rate);
    Fixture::new(params, with_tail(iq, rate, 10.0))
}

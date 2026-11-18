use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn rtty_decoder() -> Positive {
    reads(&[r#""text":"CQ CQ DE DL1ABC K""#]).hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("rtty", &RttyParams::default()).expect("rtty settings");
    let rate = Fixture::rate(&params);
    let iq = synth::transmission("CQ CQ DE DL1ABC K", 45.45, 170.0, 1.5, rate);
    Fixture::new(params, with_tail(iq, rate, 1.0))
}

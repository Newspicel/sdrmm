use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn vor_decoder() -> Positive {
    reads(&["123.1\u{b0} radial"]).hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("vor", &VorParams::default()).expect("vor settings"),
        synth::transmission(123.0, 2),
    )
}

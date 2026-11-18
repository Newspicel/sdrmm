use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn per_decoder() -> Positive {
    reads(&[r#""per":0.0"#, r#""missed":0"#, r#""bit_errors":0"#]).hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("per", &PerParams::default()).expect("per settings");
    let rate = Fixture::rate(&params);
    Fixture::new(params, synth::frames(&PerParams::default(), 0..100, rate))
}

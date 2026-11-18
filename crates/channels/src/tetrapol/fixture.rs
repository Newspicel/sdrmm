use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn tetrapol_decoder() -> Positive {
    reads(&[
        r#""group":393"#,
        r#""channel":2890"#,
        r#""country":3"#,
        r#""bs":7"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params =
        ChannelParams::new("tetrapol", &TetrapolParams::default()).expect("tetrapol settings");
    let rate = Fixture::rate(&params);
    Fixture::new(params, synth::control_channel(rate, 1))
}

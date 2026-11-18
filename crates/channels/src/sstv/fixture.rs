use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn sstv_decoder() -> Positive {
    reads(&[
        r#""mode":"martin_m1""#,
        r#""width":320"#,
        r#""height":256"#,
        r#""complete":true"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("sstv", &SstvParams::default()).expect("sstv settings");
    let mode = SstvMode::MartinM1;
    let rate = Fixture::rate(&params);
    let iq = synth::transmission(mode, &synth::bars(mode), rate);
    Fixture::new(params, with_tail(iq, rate, 2.0))
}

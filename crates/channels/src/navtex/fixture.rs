use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn navtex_decoder() -> Positive {
    reads(&[
        r#""station":"F""#,
        r#""subject":"A""#,
        r#""serial":1"#,
        r#""text":"SDR FIXTURE""#,
        r#""complete":true"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("navtex", &NavtexParams::default()).expect("navtex settings");
    let rate = Fixture::rate(&params);
    let iq = synth::transmission("ZCZC FA01 SDR FIXTURE NNNN", rate);
    Fixture::new(params, with_tail(iq, rate, 1.0))
}

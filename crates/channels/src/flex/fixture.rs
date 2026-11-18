use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn flex_decoder() -> Positive {
    reads(&[
        r#""address":1234567"#,
        r#""cycle":4"#,
        r#""frame":72"#,
        r#""text":"SDR FIXTURE""#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("flex", &FlexParams::default()).expect("flex settings");
    let page = synth::Page {
        address: 1_234_567,
        text: "SDR FIXTURE".to_owned(),
    };
    let rate = Fixture::rate(&params);
    let iq = synth::transmission(&page, 4, 72, rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

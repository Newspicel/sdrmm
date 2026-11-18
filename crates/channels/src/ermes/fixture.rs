use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn ermes_decoder() -> Positive {
    reads(&[
        r#""local_address":234567"#,
        r#""message_number":3"#,
        r#""urgent":true"#,
        r#""alert":5"#,
        r#""text":"ERMES ALPHA PAGE""#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("ermes", &ErmesParams::default()).expect("ermes settings");
    let page = synth::Page {
        local_address: 234_567,
        message_number: 3,
        text: "ERMES ALPHA PAGE".to_owned(),
        urgent: true,
        alert: 5,
    };
    let rate = Fixture::rate(&params);
    let iq = synth::transmission(&page, rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

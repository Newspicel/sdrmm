use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn wenet_decoder() -> Positive {
    reads(&[
        r#""text":"Wenet test""#,
        r#""callsign":"VK5ARG""#,
        r#""complete":true"#,
    ])
    .at(-34.95, 138.52)
    .pictures(1)
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("wenet", &WenetParams::default()).expect("wenet settings");
    let rate = Fixture::rate(&params);
    let iq = synth::transmission(WenetMode::Uart, rate);
    Fixture::new(params, with_tail(iq, rate, 0.2))
}

use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn pocsag_decoder() -> Positive {
    reads(&[
        r#""address":1234567"#,
        r#""function":3"#,
        r#""baud":1200"#,
        r#""text":"SDR FIXTURE""#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("pocsag", &PocsagParams::default()).expect("pocsag settings");
    let pages = [synth::Page {
        address: 1_234_567,
        function: 3,
        text: "SDR FIXTURE".to_owned(),
        numeric: false,
        seal: None,
    }];
    let rate = Fixture::rate(&params);
    let iq = synth::transmission(&pages, 1_200, 4_500.0, rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

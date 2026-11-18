use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn radiosonde_decoder() -> Positive {
    reads(&[
        r#""sonde":"rs41""#,
        r#""serial":"W3420567""#,
        r#""satellites":9"#,
        r#""battery_v":2.9"#,
    ])
    .at(48.1235, 11.5432)
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("radiosonde", &RadiosondeParams::default())
        .expect("radiosonde settings");
    let rate = Fixture::rate(&params);
    let iq = synth::transmission(SondeType::Rs41, 10, rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

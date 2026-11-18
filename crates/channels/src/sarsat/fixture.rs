use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn sarsat_decoder() -> Positive {
    reads(&[
        r#""identity":"MMSI 211406406""#,
        r#""beacon":"epirb""#,
        r#""country":211"#,
    ])
    .at(54.3233, 10.1233)
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("sarsat", &SarsatParams::default()).expect("sarsat settings");
    let message = synth::epirb_location(synth::EpirbFix {
        country: 211,
        mmsi_tail: 406_406,
        lat: 54.3233,
        lon: 10.1228,
    });
    let rate = Fixture::rate(&params);
    let iq = synth::transmission(&message, false, rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

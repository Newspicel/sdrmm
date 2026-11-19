use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn uat_decoder() -> Positive {
    reads(&[
        r#""icao":"A952B5""#,
        r#""callsign":"N70FC""#,
        r#""location":"KPAO""#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("uat", &UatParams::default()).expect("uat settings");
    let rate = Fixture::rate(&params);
    Fixture::new(params, synth::scene(rate)).tuned(HOME_HZ)
}

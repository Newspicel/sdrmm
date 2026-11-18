use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn adsb_decoder() -> Positive {
    reads(&[
        r#""icao":"3C65AC""#,
        r#""callsign":"DLH123""#,
        r#""vertical_rate_fpm":-1216"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("adsb", &AdsbParams::default()).expect("adsb settings");
    let icao = 0x3C_65_AC;
    let frames = vec![
        synth::squitter(icao, synth::me_identification("DLH123")),
        synth::squitter(icao, synth::me_velocity(451.0, 87.0, -1_216)),
    ];
    let rate = Fixture::rate(&params);
    let iq = synth::transmission(&frames, 500.0, 0.8, rate);
    Fixture::new(params, with_tail(iq, rate, 0.01))
}

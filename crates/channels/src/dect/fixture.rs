use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn dect_decoder() -> Positive {
    reads(&[
        r#""rfpi":"0123456780""#,
        r#""carrier":3"#,
        r#""rf_carriers":1023"#,
        r#""pscn":5"#,
        r#""authentication_supported":true"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let station = synth::Station {
        capabilities: synth::capability_bits(&[17, 33, 36, 37]),
        ..synth::Station::default()
    };
    Fixture::new(
        ChannelParams::new("dect", &DectParams::default()).expect("dect settings"),
        synth::dummy_bearer(&station, 40),
    )
}

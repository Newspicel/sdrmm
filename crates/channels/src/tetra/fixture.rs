use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn tetra_decoder() -> Positive {
    reads(&[
        r#""color_code":17"#,
        r#""network_id":1010"#,
        r#""system_id":262"#,
        r#""site_id":4242"#,
        r#""text":"Einsatz Hbf""#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("tetra", &TetraParams::default()).expect("tetra settings"),
        synth::sample(synth::Cell::default()).modulate(),
    )
}

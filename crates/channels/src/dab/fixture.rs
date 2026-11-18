use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn dab_decoder() -> Positive {
    reads(&[
        r#""service_id":49569"#,
        r#""name":"slide.png""#,
        r#""media_type":"image/png""#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("dab", &DabParams::default()).expect("dab settings"),
        synth::ensemble_for_mode(DabTransmissionMode::I, 16).expect("a synthetic signal"),
    )
}

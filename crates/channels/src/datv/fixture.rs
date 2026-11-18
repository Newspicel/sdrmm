use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn datv_decoder() -> Positive {
    reads(&[r#""protocol":2048"#, r#""name":"GSE-1-0.bin""#]).hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new(
            "datv",
            &DatvParams {
                standard: DatvStandard::DvbS2,
                symbol_rate: synth::SYMBOL_RATE,
                ..DatvParams::default()
            },
        )
        .expect("datv settings"),
        synth::dvbs2_generic(3, &[4]).expect("a synthetic signal"),
    )
}

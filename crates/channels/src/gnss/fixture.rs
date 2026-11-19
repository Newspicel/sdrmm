use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

const GNSS_FIXTURE_PRN: u8 = 7;

pub(crate) fn gnss_decoder() -> Positive {
    reads(&[
        r#""prn":7"#,
        r#""doppler_hz":1000.0"#,
        r#""code_phase_chips":158.3"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("gnss", &GnssParams::default()).expect("gnss settings"),
        synth::acquisition(GNSS_FIXTURE_PRN, 1_000.0, 317, 1_000),
    )
}

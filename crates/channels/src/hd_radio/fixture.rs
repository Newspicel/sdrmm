use num_complex::Complex;

use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_decoder_test_support::cf32_le;
use sdrmm_wire::ChannelParams;

pub(crate) fn hd_radio_decoder() -> Positive {
    reads(&[
        r#""locked":true"#,
        r#""ensemble_label":"KUT""#,
        r#""long_name":"The University of Texas at Austin""#,
        r#""label":"HD2 Public""#,
    ])
    .hearing(|| signal().heard())
}

/// KUT's hybrid FM signal, off air: 2.25 s from the nrsc5 project's sample capture.
pub(crate) fn kut() -> Vec<Complex<f32>> {
    cf32_le(include_bytes!(
        "../../../../fixtures/hd_radio/kut_558k.sigmf-data"
    ))
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("hd_radio", &HdRadioParams::default()).expect("hd radio settings"),
        kut(),
    )
}

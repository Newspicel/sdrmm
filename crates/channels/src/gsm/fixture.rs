use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_decoder_test_support::cf32_le;
use sdrmm_wire::ChannelParams;

const GSM_ARFCN_725_HZ: f64 = 1_847_800_000.0;

pub(crate) fn gsm_decoder() -> Positive {
    reads(&[r#""arfcn":725"#, r#""bsic":48"#, r#""value":"015C5703""#]).hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("gsm", &GsmParams::default()).expect("gsm settings"),
        cf32_le(include_bytes!(
            "../../../../fixtures/gsm_dcs1800_arfcn725_541k.sigmf-data"
        )),
    )
    .tuned(GSM_ARFCN_725_HZ)
}

use super::*;
use crate::builtin::fixtures::{Fixture, Positive, ci16_le, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn inmarsat_aero_decoder() -> Positive {
    reads(&[r#""station":"HL8217""#, r#""tail":"HL8217""#]).hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("inmarsat_aero", &InmarsatAeroParams::default())
            .expect("inmarsat_aero settings"),
        ci16_le(include_bytes!(
            "../../../../fixtures/inmarsat_aero_offair_48k.sigmf-data"
        )),
    )
}

use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_decoder_test_support::cf32_le;
use sdrmm_wire::ChannelParams;

pub(crate) fn lora_decoder() -> Positive {
    reads(&[
        r#""spreading_factor":7"#,
        r#""coding_rate":"4/8""#,
        r#""integrity":"crc_ok""#,
        r#""payload":"deadbeef""#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new("lora", &LoraParams::default()).expect("lora settings"),
        cf32_le(include_bytes!(
            "../../../../fixtures/lora_rn2483_sf7_250k.sigmf-data"
        )),
    )
}

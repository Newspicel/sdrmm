use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn mpt1327_decoder() -> Positive {
    reads(&[
        r#""opcode":"GTC""#,
        r#""channel":123"#,
        r#""destination_call":"10/4105""#,
        r#""system_id":8151"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::of(&Mpt1327Params::default()).expect("mpt1327 settings");
    let rate = Fixture::rate(&params);
    let iq = synth::control_channel(synth::SITE_SYSTEM, &synth::site_messages(), rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

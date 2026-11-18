use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_lead, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn morse_decoder() -> Positive {
    reads(&[
        r#""text":"CQ""#,
        r#""text":" CQ""#,
        r#""text":" DE""#,
        r#""text":" SDR""#,
        r#""text":" K""#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("morse", &MorseParams::default()).expect("morse settings");
    let rate = Fixture::rate(&params);
    let iq = sdrmm_modem::synth::morse::transmission("CQ CQ DE SDR K", 20.0, 0.0, rate);
    Fixture::new(params, with_tail(with_lead(iq, rate, 0.5), rate, 3.5))
}

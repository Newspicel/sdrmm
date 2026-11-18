use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_lead, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn psk_decoder() -> Positive {
    reads(&[r#""baud":"psk31""#, r#""text":"CQ CQ DE SDR \n""#]).hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("psk", &PskParams::default()).expect("psk settings");
    let iq =
        synth::transmission("CQ CQ DE SDR \n", PskBaud::Psk31.rate()).expect("a synthetic signal");
    let rate = Fixture::rate(&params);
    Fixture::new(params, with_tail(with_lead(iq, rate, 0.5), rate, 1.0))
}

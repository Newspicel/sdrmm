use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn dtmf_decoder() -> Positive {
    reads(&[r#""digits":"0123456789*#ABCD""#]).hearing(|| signal().expect("dtmf fixture").heard())
}

fn signal() -> Option<Fixture> {
    let params = ChannelParams::new("dtmf", &DtmfParams::default()).expect("dtmf settings");
    let rate = Fixture::rate(&params);
    let iq = synth::transmission("0123456789*#ABCD", 70, 50, rate).ok()?;
    Some(Fixture::new(params, with_tail(iq, rate, 2.0)))
}

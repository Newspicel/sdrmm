use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn selcall_decoder() -> Positive {
    reads(&[r#""system":"ccir1""#, r#""code":"12234""#])
        .hearing(|| signal().expect("selcall fixture").heard())
}

fn signal() -> Option<Fixture> {
    let params =
        ChannelParams::new("selcall", &SelcallParams::default()).expect("selcall settings");
    let rate = Fixture::rate(&params);
    let iq = synth::transmission(SelcallSystem::Ccir1, "12234", rate).ok()?;
    Some(Fixture::new(params, with_tail(iq, rate, 0.5)))
}

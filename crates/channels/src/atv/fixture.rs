use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn atv_decoder() -> Positive {
    reads(&[]).pictures(1).hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("atv", &AtvParams::default()).expect("atv settings");
    let rate = Fixture::rate(&params);
    let source = synth::AtvSource::new(&AtvParams::default(), rate);
    Fixture::new(params, synth::bars(&source, 2))
}

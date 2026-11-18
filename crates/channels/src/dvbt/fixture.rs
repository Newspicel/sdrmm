use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

const DVBT_SYMBOLS: usize = 1_024;

pub(crate) fn dvbt_decoder() -> Positive {
    reads(&[
        r#""locked":true"#,
        r#""label":"Rust TV""#,
        r#""ensemble_label":"SDRmm""#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("dvbt", &DvbtParams::default()).expect("dvbt settings");
    let rate = Fixture::rate(&params);
    let iq = synth::waveform(synth::defaults(), DVBT_SYMBOLS);
    Fixture::new(params, with_tail(iq, rate, 0.01))
}

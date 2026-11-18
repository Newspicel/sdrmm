use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn noise_figure_decoder() -> Positive {
    reads(&["Y 10.00 dB"]).hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("noise_figure", &NoiseFigureParams::default())
        .expect("noise_figure settings");
    let rate = Fixture::rate(&params);
    Fixture::new(params, synth::switched(10.0, 1.0, rate))
}

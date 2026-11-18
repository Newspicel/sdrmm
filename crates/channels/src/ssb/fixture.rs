use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use num_complex::Complex;
use sdrmm_wire::ChannelParams;

pub(crate) fn ssb_decoder() -> Positive {
    reads(&[]).audio(16_000).hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("ssb", &SsbParams::default()).expect("ssb settings");
    let rate = Fixture::rate(&params);
    let iq = (0..(rate * 0.5) as usize)
        .map(|index| {
            let phase = std::f64::consts::TAU * 1_000.0 * index as f64 / rate;
            Complex::from_polar(0.5, phase as f32)
        })
        .collect();
    Fixture::new(params, iq)
}

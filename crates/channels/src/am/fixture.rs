use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use num_complex::Complex;
use sdrmm_wire::ChannelParams;

pub(crate) fn am_decoder() -> Positive {
    reads(&[]).audio(16_000).hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("am", &AmParams::default()).expect("am settings");
    let rate = Fixture::rate(&params);
    let iq = (0..(rate * 0.5) as usize)
        .map(|index| {
            let time = index as f64 / rate;
            let envelope = f64::midpoint(1.0, 0.6 * (std::f64::consts::TAU * 1_000.0 * time).sin());
            Complex::new(envelope as f32, 0.0)
        })
        .collect();
    Fixture::new(params, iq)
}

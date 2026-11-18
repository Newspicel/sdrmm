use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use num_complex::Complex;
use sdrmm_wire::ChannelParams;

pub(crate) fn ils_decoder() -> Positive {
    reads(&[r#""component":"localizer""#, "+0.160 DDM"]).hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("ils", &IlsParams::default()).expect("ils settings");
    let rate = Fixture::rate(&params);
    let iq = (0..(2.0 * rate) as usize)
        .map(|index| {
            let time = index as f64 / rate;
            let envelope = 1.0
                + 0.28 * (std::f64::consts::TAU * 90.0 * time).cos()
                + 0.12 * (std::f64::consts::TAU * 150.0 * time).cos();
            Complex::new(envelope as f32, 0.0)
        })
        .collect();
    Fixture::new(params, iq)
}

use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

const SID_CENTRE_HZ: f64 = 30_000.0;

pub(crate) fn sid_decoder() -> Positive {
    reads(&["GQD 42 dB", "DHO38 48 dB", "NAA 38 dB"]).hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("sid", &SidParams::default()).expect("sid settings");
    let rate = Fixture::rate(&params);
    Fixture::new(
        params,
        synth::carriers(
            &[(19_580.0, 0.05), (23_400.0, 0.1), (24_000.0, 0.03)],
            SID_CENTRE_HZ,
            10.0,
            rate,
            0x51D,
        ),
    )
    .tuned(SID_CENTRE_HZ)
}

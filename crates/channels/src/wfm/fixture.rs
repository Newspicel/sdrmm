use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn wfm_decoder() -> Positive {
    reads(&[
        r#""pi":"D3C2""#,
        r#""ps":"SDR-M4""#,
        r#""radiotext":"fixture""#,
        r#""pty":10"#,
    ])
    .audio(16_000)
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("wfm", &WfmParams::default()).expect("wfm settings");
    let rate = Fixture::rate(&params);
    let station = rds::synth::Station {
        pi: 0xD3C2,
        ps: "SDR-M4  ".to_owned(),
        radiotext: "fixture".to_owned(),
        pty: 10,
        tp: true,
        ta: false,
        music: true,
        alt_freqs_hz: vec![89_800_000.0],
    };
    Fixture::new(
        params,
        rds::synth::transmission(&station, 6.0, Some(1_000.0), rate),
    )
}

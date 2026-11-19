use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn gk2a_lrit_decoder() -> Positive {
    reads(&[r#""channel":"IR105""#, r#""segments":10"#, synth::TEXT_NAME])
        .pictures(1)
        .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let key = Gk2aKey {
        index: synth::KEY_INDEX,
        key: synth::KEY
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    };
    let params =
        ChannelParams::new("gk2a_lrit", &Gk2aLritParams { keys: vec![key] }).expect("settings");
    Fixture::new(params, synth::transmission()).tuned(DOWNLINK_HZ)
}

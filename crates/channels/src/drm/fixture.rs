use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn drm_decoder() -> Positive {
    reads(&[
        r#""locked":true"#,
        r#""system":"drm30""#,
        r#""label":"Rust Wave""#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    Fixture::new(
        ChannelParams::new(
            "drm",
            &DrmParams {
                mode: DrmMode::Auto,
                bandwidth_hz: 100_000.0,
                service: None,
            },
        )
        .expect("drm settings"),
        synth::signal(synth::defaults(synth::Robustness::B), 6).expect("a synthetic signal"),
    )
}

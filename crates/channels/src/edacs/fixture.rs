use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn edacs_decoder() -> Positive {
    reads(&[
        r#""opcode":"Group call""#,
        r#""channel":9"#,
        r#""destination":765"#,
        r#""source":4660"#,
        r#""site_id":2"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let grant = synth::group_assignment(765, 4_660, 9);
    Fixture::new(
        ChannelParams::new("edacs", &EdacsParams::default()).expect("edacs settings"),
        with_tail(
            synth::control_channel(
                &[grant, (synth::site(2, 1), synth::group_update(765, 9))],
                EdacsBaud::B9600,
                INPUT_RATE_HZ,
            ),
            INPUT_RATE_HZ,
            0.2,
        ),
    )
}

use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn acars_decoder() -> Positive {
    reads(&[
        r#""registration":"D-AIBL""#,
        r#""flight":"LH0400""#,
        r#""label":"H1""#,
        r#""block_id":"4""#,
        r#""text":"SDR FIXTURE""#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("acars", &AcarsParams::default()).expect("acars settings");
    let block = synth::Block {
        mode: '2',
        registration: ".D-AIBL",
        ack: '\u{15}',
        label: "H1",
        block_id: '4',
        seq_no: Some("M01A"),
        flight: Some("LH0400"),
        text: "SDR FIXTURE",
        more: false,
    };
    let rate = Fixture::rate(&params);
    let iq = synth::transmission(&block, rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

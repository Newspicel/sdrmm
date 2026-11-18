use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn ident_decoder() -> Positive {
    reads(&[r#""type_id":"pocsag""#, r#""confirmed":true"#]).hearing(|| signal().heard())
}

/// Pages a pager sends over and over, taken from the signal catalog so the identifier is tried
/// on another decoder's signal without naming it.
fn signal() -> Fixture {
    let params = ChannelParams::new(
        "ident",
        &IdentParams {
            interval_ms: 500,
            ..IdentParams::default()
        },
    )
    .expect("ident settings");
    let rate = Fixture::rate(&params);
    let pager = sdrmm_decoder::signal_catalog()
        .find("pocsag")
        .expect("a pager signal");
    let burst = sdrmm_modem::synth::resample(
        &pager.rendered().expect("a synthetic signal"),
        pager.rate_hz,
        rate,
    );
    let mut iq = Vec::new();
    while iq.len() < (2.0 * rate) as usize {
        iq.extend_from_slice(&burst);
    }
    sdrmm_modem::synth::add_noise(&mut iq, 0x5d90, 0.01);
    Fixture::new(params, iq)
}

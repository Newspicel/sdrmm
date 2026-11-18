use super::CwSkimmerParams;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

pub(crate) fn cw_skimmer_decoder() -> Positive {
    reads(&["DL1AAA", "20 WPM"]).hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new(
        "cw_skimmer",
        &CwSkimmerParams {
            bandwidth_hz: 16_000.0,
            ..CwSkimmerParams::default()
        },
    )
    .expect("cw_skimmer settings");
    let rate = Fixture::rate(&params);
    let mut iq = sdrmm_modem::synth::morse::hard_keyed(
        "VVV VVV CQ DE DL1AAA K VVV VVV CQ DE DL1AAA K",
        20.0,
        3_500.0,
        rate,
    );
    sdrmm_modem::synth::scale(&mut iq, 10f32.powf(55.0 / 20.0));
    sdrmm_modem::synth::add_noise(&mut iq, 7, 1.0);
    Fixture::new(params, iq)
}

use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

const BLE_38_HZ: f64 = 2_426e6;

pub(crate) fn ble_decoder() -> Positive {
    reads(&[
        r#""name":"Greenhouse""#,
        r#""company":"Apple""#,
        r#""url":"https://sdrmm.com""#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new(
        "ble",
        &BleParams {
            link: BleLink::Channel,
        },
    )
    .expect("ble settings");
    let rate = Fixture::rate(&params);
    Fixture::new(params, synth::scene(rate)).tuned(BLE_38_HZ)
}

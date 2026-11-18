use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads};
use sdrmm_wire::ChannelParams;

const BLE_37_HZ: f64 = 2_402e6;

pub(crate) fn remote_id_decoder() -> Positive {
    reads(&[
        r#""uas_id":"1581F5FJD239C00DW22E""#,
        r#""ua_type":"rotorcraft""#,
    ])
    .at(52.5163, 13.3777)
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new(
        "remote_id",
        &RemoteIdParams {
            link: RemoteIdLink::Bluetooth,
        },
    )
    .expect("remote_id settings");
    let rate = Fixture::rate(&params);
    Fixture::new(params, synth::bluetooth_scene(rate)).tuned(BLE_37_HZ)
}

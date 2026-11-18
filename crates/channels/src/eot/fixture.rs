use super::*;
use crate::builtin::fixtures::{Fixture, Positive, reads, with_tail};
use sdrmm_wire::ChannelParams;

pub(crate) fn eot_decoder() -> Positive {
    reads(&[
        r#""unit_address":24680"#,
        r#""arming":"normal""#,
        r#""pressure_psig":90"#,
        r#""battery_charge_pct":76"#,
    ])
    .hearing(|| signal().heard())
}

fn signal() -> Fixture {
    let params = ChannelParams::new("eot", &EotParams::default()).expect("eot settings");
    let rear = synth::Rear {
        unit_address: 24_680,
        status: EotStatus {
            message_type: 0,
            arming: EotArming::Normal,
            pressure_psig: 90,
            battery: EotBattery::Ok,
            battery_charge_pct: 76,
            valve_ok: true,
            confirmed: false,
            turbine: true,
            motion: true,
            marker_light: true,
            marker_battery_low: false,
            discretionary: false,
            chaining: 3,
        },
    };
    let rate = Fixture::rate(&params);
    let iq = synth::rear_transmission(&rear, rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

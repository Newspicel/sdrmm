use super::*;
use crate::builtin::fixtures::{Heard, Positive, reads, scene_heard};
use sdrmm_decoder_test_support::scene::Scene;
use sdrmm_wire::{AirtimeSpan, ChannelParams};

const WIFI_6_HZ: f64 = 2_437e6;

pub(crate) fn wifi_occupancy_decoder() -> Positive {
    reads(&[r#""number":6"#, r#""band":"ghz2_4""#, "ch 6 30%"]).hearing(signal)
}

fn signal() -> Heard {
    let span = AirtimeSpan::Mhz20;
    let rate = span.sample_rate_hz();
    let frame = sdrmm_modem::wifi::transmit::filler_frame(300);
    let on_s = frame.len() as f64 / rate;
    let mut scene = Scene::new(rate, 0.01);
    scene.every(on_s / 0.3, 1.01, 0.0, 0.1, &frame);
    let params = ChannelParams::new(
        "wifi_occupancy",
        &WifiOccupancyParams {
            span,
            ..WifiOccupancyParams::default()
        },
    )
    .expect("wifi_occupancy settings");
    scene_heard(params, WIFI_6_HZ, &scene)
}

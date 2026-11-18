use super::*;
use crate::builtin::fixtures::{Heard, Positive, reads, scene_heard};
use sdrmm_decoder::input_rate;
use sdrmm_decoder_test_support::scene::Scene;
use sdrmm_wire::ChannelParams;

const WIFI_6_HZ: f64 = 2_437e6;

pub(crate) fn ism_survey_decoder() -> Positive {
    reads(&[r#""kind":"wifi""#]).hearing(signal)
}

fn signal() -> Heard {
    let params =
        ChannelParams::new("ism_survey", &IsmSurveyParams::default()).expect("ism_survey settings");
    let rate = input_rate(&params);
    let frame = sdrmm_modem::wifi::transmit::filler_frame(300);
    let mut scene = Scene::new(rate, 0.01);
    scene.every(0.01, 1.01, 0.0, 0.1, &frame);
    scene_heard(params, WIFI_6_HZ, &scene)
}

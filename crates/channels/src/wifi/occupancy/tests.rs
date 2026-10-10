use sdrmm_wire::{
    AirtimeSpan, ChannelParams, ChannelSettings, DecoderEvent, WifiOccupancyParams,
    WifiOccupancyReport,
};

use super::WifiOccupancyChannel;
use crate::{
    ChannelCtx, ChannelRx,
    airtime::scene::Scene,
    synth::{self, ism::wifi_frame},
    testutil::settings,
};

const CHANNEL_6_HZ: f64 = 2_437e6;

fn channel(span: AirtimeSpan, frequency_hz: f64) -> WifiOccupancyChannel {
    let params = WifiOccupancyParams {
        span,
        ..WifiOccupancyParams::default()
    };
    let tuned = ChannelSettings {
        frequency_hz,
        ..settings(ChannelParams::WifiOccupancy(params))
    };
    WifiOccupancyChannel::new(
        ChannelCtx {
            input_rate: span.sample_rate_hz(),
        },
        tuned,
    )
    .expect("builds")
}

fn reports(scene: &Scene, channel: &mut WifiOccupancyChannel) -> Vec<WifiOccupancyReport> {
    scene
        .run(1.01, channel)
        .events
        .into_iter()
        .filter_map(|event| match event {
            DecoderEvent::WifiOccupancy(report) => Some(report),
            _ => None,
        })
        .collect()
}

#[test]
fn a_twenty_megahertz_window_measures_the_channel_it_sits_on() {
    let rate = AirtimeSpan::Mhz20.sample_rate_hz();
    let frame = wifi_frame(300);
    let on_s = frame.len() as f64 / rate;
    let mut scene = Scene::new(rate, 0.01);
    scene.every(on_s / 0.3, 1.01, 0.0, 0.1, &frame);
    let mut channel = channel(AirtimeSpan::Mhz20, CHANNEL_6_HZ);
    let reports = reports(&scene, &mut channel);
    assert_eq!(reports.len(), 1);
    let report = &reports[0];
    assert!((report.measured - 1.0).abs() < 1e-3);
    assert_eq!(report.channels.len(), 1);
    let load = &report.channels[0];
    assert_eq!(load.number, 6);
    assert!((0.27..0.34).contains(&load.busy), "{load:?}");
    let floor = load.floor_dbfs.unwrap();
    let level = load.level_dbfs.unwrap();
    assert!(level - floor > 15.0, "{load:?}");
}

#[test]
fn silence_from_a_squelch_is_not_measured() {
    let scene = Scene::new(AirtimeSpan::Mhz20.sample_rate_hz(), 0.0);
    let mut channel = channel(AirtimeSpan::Mhz20, CHANNEL_6_HZ);
    let report = &reports(&scene, &mut channel)[0];
    assert_eq!(report.measured, 0.0);
    assert_eq!(report.channels[0].busy, 0.0);
    assert_eq!(report.channels[0].floor_dbfs, None);
}

#[test]
fn a_forty_megahertz_window_splits_load_by_channel() {
    let rate = AirtimeSpan::Mhz40.sample_rate_hz();
    let frame = synth::resample(&wifi_frame(300), 20e6, rate);
    let on_s = frame.len() as f64 / rate;
    let mut scene = Scene::new(rate, 0.01);
    scene.every(on_s / 0.5, 1.01, -10e6, 0.1, &frame);
    let mut channel = channel(AirtimeSpan::Mhz40, 2_442e6);
    let report = &reports(&scene, &mut channel)[0];
    let numbers: Vec<u8> = report.channels.iter().map(|load| load.number).collect();
    assert_eq!(numbers, vec![5, 6, 7, 8, 9]);
    let busy = |number: u8| {
        report
            .channels
            .iter()
            .find(|load| load.number == number)
            .map(|load| load.busy)
            .unwrap()
    };
    assert!((0.45..0.56).contains(&busy(5)), "{report:?}");
    assert!(busy(9) < 0.02, "{report:?}");
}

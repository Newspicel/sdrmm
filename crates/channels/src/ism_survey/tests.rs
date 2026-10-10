use sdrmm_dsp::airtime::Burst;
use sdrmm_wire::{
    AirtimeSpan, ChannelParams, ChannelSettings, DecoderEvent, IsmKind, IsmSurveyParams,
    IsmSurveyReport,
};

use super::{IsmSurveyChannel, tally::Tally};
use crate::{
    ChannelCtx, ChannelRx,
    airtime::scene::Scene,
    synth::ism::{ble_advert, oven_burst, wifi_frame, zigbee_frame},
    testutil::settings,
};

const CENTRE_HZ: f64 = 2_437e6;
const RATE: f64 = 20e6;

fn report(scene: &Scene) -> IsmSurveyReport {
    let tuned = ChannelSettings {
        frequency_hz: CENTRE_HZ,
        ..settings(ChannelParams::IsmSurvey(IsmSurveyParams::default()))
    };
    let mut channel =
        IsmSurveyChannel::new(ChannelCtx { input_rate: RATE }, tuned).expect("builds");
    let mut reports: Vec<IsmSurveyReport> = scene
        .run(1.01, &mut channel)
        .events
        .into_iter()
        .filter_map(|event| match event {
            DecoderEvent::IsmSurvey(report) => Some(report),
            _ => None,
        })
        .collect();
    assert_eq!(reports.len(), 1);
    reports.remove(0)
}

fn bursts(report: &IsmSurveyReport, kind: IsmKind) -> u32 {
    report
        .kinds
        .iter()
        .find(|load| load.kind == kind)
        .map_or(0, |load| load.bursts)
}

fn starts(first_s: f64, period_s: f64) -> impl Iterator<Item = f64> {
    (0..)
        .map(move |k| first_s + f64::from(k) * period_s)
        .take_while(|&t| t < 1.0)
}

#[test]
fn noise_alone_is_quiet() {
    let report = report(&Scene::new(RATE, 0.01));
    let total: u32 = report.kinds.iter().map(|load| load.bursts).sum();
    assert!(total <= 2, "{report:?}");
    assert!(report.busy < 0.01, "{report:?}");
    assert!(report.floor_dbfs.is_some());
    assert_eq!(report.dropped, 0);
}

#[test]
fn wifi_bluetooth_and_802_15_4_are_told_apart() {
    let mut scene = Scene::new(RATE, 0.01);
    let wifi = wifi_frame(300);
    let ble = ble_advert(RATE);
    let zigbee = zigbee_frame(RATE, 1e-3);
    for start in starts(0.010, 0.020) {
        scene.at(start, 0.0, 0.1, wifi.clone());
    }
    for start in starts(0.0155, 0.030) {
        scene.at(start, 3e6, 0.1, ble.clone());
    }
    for start in starts(0.0035, 0.050) {
        scene.at(start, -2e6, 0.1, zigbee.clone());
    }
    let report = report(&scene);
    let near = |found: u32, wanted: u32| found.abs_diff(wanted) <= 1;
    assert!(near(bursts(&report, IsmKind::Wifi), 44), "{report:?}");
    assert!(near(bursts(&report, IsmKind::Bluetooth), 29), "{report:?}");
    assert!(near(bursts(&report, IsmKind::Ieee802154), 17), "{report:?}");
    let ble = report
        .kinds
        .iter()
        .find(|load| load.kind == IsmKind::Bluetooth)
        .unwrap();
    assert_eq!(ble.centres_mhz[0], 2_440.0);
    assert!((150.0..400.0).contains(&ble.mean_us), "{ble:?}");
    let wifi = report
        .kinds
        .iter()
        .find(|load| load.kind == IsmKind::Wifi)
        .unwrap();
    assert_eq!(wifi.centres_mhz[0], 2_437.0);
}

#[test]
fn a_steady_carrier_is_continuous() {
    let mut scene = Scene::new(RATE, 0.01);
    scene.carrier(6.5e6, 0.05);
    let report = report(&scene);
    let steady = report
        .kinds
        .iter()
        .find(|load| load.kind == IsmKind::Continuous)
        .expect("a continuous entry");
    assert_eq!(steady.airtime, 1.0);
    assert!(steady.centres_mhz.contains(&2_443.0) || steady.centres_mhz.contains(&2_444.0));
    assert_eq!(report.busy, 1.0);
}

#[test]
fn an_oven_pulses_with_the_mains() {
    let mut scene = Scene::new(RATE, 0.01);
    let burst = oven_burst(RATE, 9e-3);
    for start in starts(0.005, 0.020) {
        scene.at(start, 4e6, 1.0, burst.clone());
    }
    let report = report(&scene);
    assert!(bursts(&report, IsmKind::MicrowaveOven) >= 40, "{report:?}");
    let oven = report
        .kinds
        .iter()
        .find(|load| load.kind == IsmKind::MicrowaveOven)
        .unwrap();
    assert!((0.35..0.5).contains(&oven.airtime), "{oven:?}");
}

fn burst(first: u64, frames: u64) -> Burst {
    Burst {
        first,
        frames,
        low: 10,
        high: 20,
        centre: 15.0,
        mean: 1.0,
        peak: 1.0,
        edge: false,
        cut: false,
    }
}

#[test]
fn a_short_burst_closing_inside_a_long_one_keeps_the_long_one_whole() {
    let mut tally = Tally::new(CENTRE_HZ, AirtimeSpan::Mhz20);
    tally.add(&burst(300, 118), IsmKind::Bluetooth);
    tally.add(&burst(100, 626), IsmKind::Wifi);
    tally.add(&burst(200, 50), IsmKind::Wifi);
    assert!((tally.busy(1_000) - 0.626).abs() < 1e-6);
    let loads = tally.report(1_000);
    let airtime = |kind| {
        loads
            .iter()
            .find(|load| load.kind == kind)
            .map(|load| load.airtime)
    };
    assert!(airtime(IsmKind::Wifi).is_some_and(|share| (share - 0.626).abs() < 1e-6));
    assert!(airtime(IsmKind::Bluetooth).is_some_and(|share| (share - 0.118).abs() < 1e-6));
    tally.add(&burst(600, 300), IsmKind::Wifi);
    assert!((tally.busy(1_000) - 0.174).abs() < 1e-6);
}

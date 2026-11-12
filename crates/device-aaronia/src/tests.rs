use std::sync::mpsc;

use sdrmm_device::{LaneEvent, Sample, SinkItem};
use sdrmm_wire::{BandwidthSetting, Duplex, ExtraValue, GainKind, GainValue};

use super::*;
use crate::{
    caps::{CHANNEL, CLOCK, GAIN, SPAN, TX_GAIN, ZERO_DBM_VOLTS, tx_scale},
    testing::{FakeApi, SEGMENT, SEGMENT_END},
};

const WAIT: Duration = Duration::from_secs(5);

fn driver(api: &Arc<FakeApi>) -> AaroniaDriver {
    AaroniaDriver::with_api(api.clone())
}

fn open(api: &Arc<FakeApi>) -> Box<dyn SdrDevice> {
    let driver = driver(api);
    let info = driver.probe().into_iter().next().expect("listed");
    driver.open(&info).expect("open")
}

fn tone(volts: f32, samples: usize) -> Vec<f32> {
    std::iter::repeat_n([volts, -volts], samples)
        .flatten()
        .collect()
}

#[test]
fn a_spectran_is_listed_once_by_serial() {
    let api = Arc::new(FakeApi::v6());
    let listed = driver(&api).probe();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].driver, DRIVER_ID);
    assert_eq!(listed[0].key, api.serial());
    assert_eq!(listed[0].label, format!("SPECTRAN V6 {}", api.serial()));
}

#[test]
fn a_search_only_rescans_while_nothing_is_open() {
    let api = Arc::new(FakeApi::v6());
    let _ = driver(&api).probe();
    assert_eq!(api.rescans(), 1);
    let _device = open(&api);
    let before = api.rescans();
    let _ = driver(&api).probe();
    assert_eq!(api.rescans(), before, "an open SPECTRAN is never rescanned");
}

#[test]
fn opening_learns_the_transmitter_and_stays_open_as_a_receiver() {
    let api = Arc::new(FakeApi::v6());
    let device = open(&api);
    assert_eq!(api.opens(), vec![Mode::Transmitter, Mode::Receiver]);
    assert_eq!(api.open_count(), 1);
    let caps = device.capabilities();
    assert_eq!(caps.duplex, Duplex::Half);
    assert_eq!(caps.antennas, vec!["Rx1", "Rx2"]);
    assert!(caps.stage(GainKind::Tx.name()).is_some());
    assert_eq!(device.settings().sample_rate, Some(2e6));
    assert_eq!(api.float(Mode::Receiver, SPAN), 2e6 / caps::RATE_PER_SPAN);
}

#[test]
fn a_receiver_only_spectran_does_not_offer_transmit() {
    let api = Arc::new(FakeApi::receiver_only());
    let mut device = open(&api);
    assert_eq!(device.capabilities().duplex, Duplex::RxOnly);
    assert_eq!(device.capabilities().antennas, vec![caps::SINGLE_INPUT]);
    assert!(matches!(
        device.tx_start(),
        Err(DeviceError::Unsupported(_))
    ));
}

#[test]
fn a_second_open_reports_the_radio_in_use() {
    let api = Arc::new(FakeApi::v6());
    let _first = open(&api);
    let driver = driver(&api);
    let info = driver.probe().into_iter().next().expect("listed");
    assert!(matches!(driver.open(&info), Err(DeviceError::InUse(_))));
}

#[test]
fn a_busy_radio_is_reported_in_use_before_anything_is_read() {
    let api = Arc::new(FakeApi::v6());
    api.mark_busy();
    let driver = driver(&api);
    let info = driver.probe().into_iter().next().expect("listed");
    assert!(matches!(driver.open(&info), Err(DeviceError::InUse(_))));
}

#[test]
fn opening_a_serial_that_is_gone_reports_not_found() {
    let api = Arc::new(FakeApi::v6());
    let info = DeviceInfo {
        driver: DRIVER_ID,
        key: "999".to_string(),
        label: "gone".to_string(),
        serial: Some("999".to_string()),
        profile: None,
    };
    assert!(matches!(
        driver(&api).open(&info),
        Err(DeviceError::NotFound(_))
    ));
}

#[test]
fn tuning_rate_gain_and_input_reach_the_receiver_tree() {
    let api = Arc::new(FakeApi::v6());
    let mut device = open(&api);
    device
        .apply(&DeviceSettings {
            center_hz: Some(433.92e6),
            sample_rate: Some(30e6),
            antenna: Some("Rx2".to_string()),
            gains: vec![GainValue {
                stage: GAIN.to_string(),
                value_db: 15.0,
            }],
            ..DeviceSettings::default()
        })
        .expect("apply");
    assert_eq!(api.float(Mode::Receiver, caps::CENTER), 433.92e6);
    assert_eq!(api.float(Mode::Receiver, SPAN), 20e6);
    assert_eq!(api.text(Mode::Receiver, CLOCK), "46MHz");
    assert_eq!(api.text(Mode::Receiver, CHANNEL), "Rx2");
    assert_eq!(api.float(Mode::Receiver, caps::REFERENCE), -15.0);
    assert_eq!(device.settings().sample_rate, Some(30e6));
    assert_eq!(device.settings().gain(GAIN), Some(15.0));
}

#[test]
fn an_extra_writes_its_path_and_a_foreign_option_is_refused() {
    let api = Arc::new(FakeApi::v6());
    let mut device = open(&api);
    let extra = |value: &str| DeviceSettings {
        extra: vec![ExtraValue {
            name: "preamp".to_string(),
            value: serde_json::json!(value),
        }],
        ..DeviceSettings::default()
    };
    device.apply(&extra("Amp")).expect("on the menu");
    assert_eq!(api.text(Mode::Receiver, "calibration/preamp"), "Amp");
    assert!(matches!(
        device.apply(&extra("Turbo")),
        Err(DeviceError::Unsupported(_))
    ));
    assert_eq!(api.text(Mode::Receiver, "calibration/preamp"), "Amp");
}

#[test]
fn settings_the_spectran_lacks_are_refused() {
    let api = Arc::new(FakeApi::v6());
    let mut device = open(&api);
    for refused in [
        DeviceSettings {
            bandwidth: Some(BandwidthSetting::Manual { hz: 1e6 }),
            ..DeviceSettings::default()
        },
        DeviceSettings {
            bias_tee: Some(true),
            ..DeviceSettings::default()
        },
        DeviceSettings {
            gains: vec![GainValue {
                stage: GAIN.to_string(),
                value_db: 90.0,
            }],
            ..DeviceSettings::default()
        },
        DeviceSettings {
            sample_rate: Some(1e9),
            ..DeviceSettings::default()
        },
    ] {
        assert!(
            matches!(device.apply(&refused), Err(DeviceError::Unsupported(_))),
            "{refused:?}"
        );
    }
}

#[test]
fn samples_reach_the_sink_with_the_reference_level_at_full_scale() {
    let api = Arc::new(FakeApi::v6());
    let mut device = open(&api);
    device
        .apply(&DeviceSettings {
            gains: vec![GainValue {
                stage: GAIN.to_string(),
                value_db: 0.0,
            }],
            ..DeviceSettings::default()
        })
        .expect("0 dBm reference");
    let (tx, rx) = mpsc::channel();
    device
        .rx_start(vec![RxSink::new(move |samples: &[Sample], _| {
            tx.send(samples.to_vec()).expect("receiver lives");
        })])
        .expect("start");
    assert!(api.is_streaming());
    api.push(1.0, 2e6, tone(ZERO_DBM_VOLTS as f32, 64));
    let block = rx.recv_timeout(WAIT).expect("a block");
    assert_eq!(block.len(), 64);
    assert!((block[0].re - 1.0).abs() < 1e-5 && (block[0].im + 1.0).abs() < 1e-5);
    assert!(api.wait_until_drained(WAIT), "the packet is consumed");
    device.rx_stop();
    assert!(!api.is_streaming());
}

#[test]
fn a_hole_in_the_packet_times_steps_the_sample_index() {
    let api = Arc::new(FakeApi::v6());
    let mut device = open(&api);
    let (tx, rx) = mpsc::channel();
    device
        .rx_start(vec![RxSink::with_items(
            move |item| {
                let seen = match item {
                    SinkItem::Samples { samples, index } => Some((index, samples.len())),
                    SinkItem::Event(LaneEvent::Uncertain { .. }) => None,
                    SinkItem::Event(_) => return,
                };
                tx.send(seen).expect("receiver lives");
            },
            |_| {},
        )])
        .expect("start");
    api.push(10.0, 2e6, tone(0.0, 100));
    api.push(10.0 + 300.0 / 2e6, 2e6, tone(0.0, 100));
    assert_eq!(rx.recv_timeout(WAIT).expect("first"), Some((0, 100)));
    assert_eq!(rx.recv_timeout(WAIT).expect("gap"), None);
    assert_eq!(rx.recv_timeout(WAIT).expect("second"), Some((300, 100)));
    device.rx_stop();
}

#[test]
fn an_unplugged_spectran_fails_the_sink_as_disconnected() {
    let api = Arc::new(FakeApi::v6());
    let mut device = open(&api);
    let (tx, rx) = mpsc::channel();
    device
        .rx_start(vec![RxSink::with_fatal_handler(
            |_, _| {},
            move |error| tx.send(error).expect("receiver lives"),
        )])
        .expect("start");
    api.unplug();
    assert!(matches!(
        rx.recv_timeout(WAIT).expect("reported"),
        DeviceError::Disconnected(_)
    ));
}

#[test]
fn a_burst_is_sent_centred_on_the_tuning_and_the_receiver_comes_back_after() {
    let api = Arc::new(FakeApi::v6());
    let mut device = open(&api);
    device
        .apply(&DeviceSettings {
            center_hz: Some(145e6),
            sample_rate: Some(1e6),
            gains: vec![GainValue::new(GainKind::Tx, -10.0)],
            ..DeviceSettings::default()
        })
        .expect("tune");
    let mut stream = device.tx_start().expect("transmit");
    assert_eq!(api.opens().last(), Some(&Mode::Transmitter));
    assert_eq!(api.float(Mode::Transmitter, caps::CENTER), 145e6);
    assert_eq!(api.float(Mode::Transmitter, TX_GAIN), -10.0);
    assert!(api.is_streaming());
    let samples = vec![Sample::new(1.0, 0.0); 100];
    assert_eq!(
        stream.write(&samples, WAIT, true).expect("written"),
        samples.len()
    );
    let sent = api.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].flags, SEGMENT | SEGMENT_END);
    assert_eq!(sent[0].start_hz, 145e6 - 0.5e6);
    assert_eq!(sent[0].step_hz, 1e6);
    assert_eq!(sent[0].iq[0], tx_scale());
    stream.stop().expect("stop");
    assert!(!api.is_streaming());

    device
        .rx_start(vec![RxSink::new(|_, _| {})])
        .expect("receive again");
    assert_eq!(api.opens().last(), Some(&Mode::Receiver));
    assert_eq!(api.float(Mode::Receiver, caps::CENTER), 145e6);
    device.rx_stop();
}

#[test]
fn transmit_waits_for_receive_to_stop() {
    let api = Arc::new(FakeApi::v6());
    let mut device = open(&api);
    device
        .rx_start(vec![RxSink::new(|_, _| {})])
        .expect("start");
    assert!(matches!(
        device.tx_start(),
        Err(DeviceError::DuplexConflict { .. })
    ));
    device.rx_stop();
}

#[test]
fn readouts_come_from_the_health_tree_while_streaming() {
    let api = Arc::new(FakeApi::v6());
    let mut device = open(&api);
    assert!(device.readouts().expect("idle").is_empty());
    device
        .rx_start(vec![RxSink::new(|_, _| {})])
        .expect("start");
    let readouts = device.readouts().expect("streaming");
    assert_eq!(readouts.len(), 1);
    assert_eq!(readouts[0].name, "fpga_temp");
    device.rx_stop();
}

#[test]
fn health_names_the_library_and_version_it_found() {
    let check = health(&RuntimeInfo {
        version: Some((3 << 16) | 16745),
        library: Some("/opt/aaronia-rtsa-suite/libAaroniaRTSAAPI.so".to_string()),
        error: None,
    });
    assert_eq!(check.status, CheckStatus::Ok);
    assert!(check.detail.contains("3.16745"));
    assert!(check.hint.is_none());
}

#[test]
fn a_missing_api_warns_and_says_where_to_get_it() {
    let check = health(&RuntimeInfo {
        version: None,
        library: None,
        error: Some("the Aaronia RTSA-Suite PRO is not installed".to_string()),
    });
    assert_eq!(check.status, CheckStatus::Warn);
    assert!(
        check
            .hint
            .is_some_and(|hint| hint.contains("aaronia.com") && hint.contains(OVERRIDE_VAR))
    );
}

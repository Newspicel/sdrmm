use std::{sync::mpsc, time::Duration};

use sdrmm_device::{LaneEvent, Sample, SinkItem};
use sdrmm_wire::{BandwidthSetting, Duplex, ExtraValue};

use super::*;
use crate::{
    caps::ZERO_DBM_VOLTS,
    model::FrequencySource,
    testing::{Call, FakeApi, RSA306, TIMESTAMP_RATE},
};

const WAIT: Duration = Duration::from_secs(5);

fn driver(api: &Arc<FakeApi>) -> TektronixDriver {
    TektronixDriver::with_api(api.clone())
}

fn open(api: &Arc<FakeApi>) -> Box<dyn SdrDevice> {
    let driver = driver(api);
    let info = driver.probe().into_iter().next().expect("listed");
    driver.open(&info).expect("open")
}

fn gain(value_db: f64) -> DeviceSettings {
    DeviceSettings {
        gains: vec![GainValue {
            stage: GAIN.to_string(),
            value_db,
        }],
        ..DeviceSettings::default()
    }
}

fn preamp(on: bool) -> DeviceSettings {
    DeviceSettings {
        extra: vec![ExtraValue {
            name: caps::PREAMP.to_string(),
            value: on.into(),
        }],
        ..DeviceSettings::default()
    }
}

fn attenuation(auto: bool, value_db: f64) -> DeviceSettings {
    DeviceSettings {
        gains: vec![GainValue {
            stage: caps::ATTENUATOR.to_string(),
            value_db,
        }],
        extra: vec![ExtraValue {
            name: caps::AUTO_ATTENUATION.to_string(),
            value: auto.into(),
        }],
        ..DeviceSettings::default()
    }
}

fn source(source: FrequencySource) -> DeviceSettings {
    DeviceSettings {
        extra: vec![ExtraValue {
            name: caps::FREQUENCY_SOURCE.to_string(),
            value: source.name().into(),
        }],
        ..DeviceSettings::default()
    }
}

fn offers(device: &dyn SdrDevice, name: &str) -> bool {
    device
        .capabilities()
        .extra
        .iter()
        .any(|extra| extra.name() == name)
}

fn held(device: &dyn SdrDevice, wanted: &DeviceSettings) -> bool {
    wanted
        .extra
        .iter()
        .all(|extra| device.settings().extra.contains(extra))
}

fn tone(volts: f32, samples: usize) -> Vec<f32> {
    std::iter::repeat_n([volts, -volts], samples)
        .flatten()
        .collect()
}

#[test]
fn the_driver_claims_the_analysers_on_the_bus() {
    let spec = TektronixDriver::new().spec();
    assert_eq!(spec.backend, DRIVER_ID);
    assert!(spec.usb.iter().any(|usb| usb.matches(0x0699, 0xa306)));
    assert!(spec.usb.iter().any(|usb| usb.matches(0x0699, 0xa500)));
    assert!(!spec.usb.iter().any(|usb| usb.matches(0x0699, 0x0401)));
}

#[test]
fn an_analyser_is_listed_once_by_serial() {
    let api = Arc::new(FakeApi::rsa306());
    let listed = driver(&api).probe();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].driver, DRIVER_ID);
    assert_eq!(listed[0].key, api.serial());
    assert_eq!(
        listed[0].label,
        format!("Tektronix RSA306 {}", api.serial())
    );
}

#[test]
fn opening_connects_and_reads_what_the_analyser_can_do() {
    let api = Arc::new(FakeApi::rsa306());
    let device = open(&api);
    assert!(api.is_connected());
    let caps = device.capabilities();
    assert_eq!(caps.duplex, Duplex::RxOnly);
    assert_eq!(caps.sample_rates.len(), 13);
    assert_eq!(caps.freq_ranges[0].max, RSA306.center_max);
    assert_eq!(device.settings().sample_rate, Some(1.75e6));
    assert_eq!(device.settings().gain(GAIN), Some(20.0));
    drop(device);
    assert!(!api.is_connected(), "dropping the device lets the API go");
}

#[test]
fn a_second_open_reports_the_analyser_in_use() {
    let api = Arc::new(FakeApi::rsa306());
    let _first = open(&api);
    let driver = driver(&api);
    let info = driver.probe().into_iter().next().expect("listed");
    assert!(matches!(driver.open(&info), Err(DeviceError::InUse(_))));
}

#[test]
fn opening_a_serial_that_is_gone_reports_not_found() {
    let api = Arc::new(FakeApi::rsa306());
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
fn tuning_rate_and_gain_reach_the_analyser() {
    let api = Arc::new(FakeApi::rsa306());
    let mut device = open(&api);
    api.take_calls();
    device
        .apply(&DeviceSettings {
            center_hz: Some(433.92e6),
            sample_rate: Some(14e6),
            ..gain(35.0)
        })
        .expect("apply");
    assert_eq!(
        api.take_calls(),
        vec![
            Call::Center(433.92e6),
            Call::Reference(-35.0),
            Call::Bandwidth(14e6 * caps::BANDWIDTH_PER_RATE),
        ]
    );
    assert_eq!(device.settings().center_hz, Some(433.92e6));
    assert_eq!(device.settings().sample_rate, Some(14e6));
    assert_eq!(device.settings().gain(GAIN), Some(35.0));
}

#[test]
fn a_setting_sent_again_never_touches_the_analyser() {
    let api = Arc::new(FakeApi::rsa306());
    let mut device = open(&api);
    let held = device.settings().clone();
    api.take_calls();
    device
        .apply(&DeviceSettings {
            center_hz: held.center_hz,
            sample_rate: held.sample_rate,
            gains: held.gains,
            ..DeviceSettings::default()
        })
        .expect("apply");
    assert!(api.take_calls().is_empty());
}

#[test]
fn a_retune_while_streaming_pauses_the_acquisition_and_keeps_the_stream() {
    let api = Arc::new(FakeApi::rsa306());
    let mut device = open(&api);
    device
        .rx_start(vec![RxSink::new(|_, _| {})])
        .expect("start");
    api.take_calls();
    device
        .apply(&DeviceSettings {
            center_hz: Some(100e6),
            ..DeviceSettings::default()
        })
        .expect("retune");
    assert_eq!(
        api.take_calls(),
        vec![Call::Stop, Call::Center(100e6), Call::Run]
    );
    assert!(api.is_streaming());
    device.rx_stop();
    assert!(!api.is_streaming());
}

#[test]
fn a_gain_change_while_streaming_rebuilds_the_stream_so_its_scale_follows() {
    let api = Arc::new(FakeApi::rsa306());
    let mut device = open(&api);
    device
        .rx_start(vec![RxSink::new(|_, _| {})])
        .expect("start");
    api.take_calls();
    device.apply(&gain(45.0)).expect("gain");
    assert_eq!(
        api.take_calls(),
        vec![
            Call::StreamStop,
            Call::Stop,
            Call::Reference(-45.0),
            Call::Run,
            Call::StreamStart,
        ]
    );
    device.rx_stop();
}

#[test]
fn a_rate_change_while_streaming_rebuilds_the_stream() {
    let api = Arc::new(FakeApi::rsa306());
    let mut device = open(&api);
    device
        .rx_start(vec![RxSink::new(|_, _| {})])
        .expect("start");
    api.take_calls();
    device
        .apply(&DeviceSettings {
            sample_rate: Some(56e6),
            ..DeviceSettings::default()
        })
        .expect("rate");
    assert_eq!(
        api.take_calls(),
        vec![
            Call::StreamStop,
            Call::Stop,
            Call::Bandwidth(56e6 * caps::BANDWIDTH_PER_RATE),
            Call::Run,
            Call::StreamStart,
        ]
    );
    assert_eq!(device.settings().sample_rate, Some(56e6));
    device.rx_stop();
}

#[test]
fn a_slow_rate_first_streams_the_fastest_slow_rate_to_size_the_api_buffer() {
    let api = Arc::new(FakeApi::rsa306());
    let mut device = open(&api);
    let slowest = device.capabilities().sample_rates[0];
    device
        .apply(&DeviceSettings {
            sample_rate: Some(slowest),
            ..DeviceSettings::default()
        })
        .expect("rate");
    api.take_calls();
    device
        .rx_start(vec![RxSink::new(|_, _| {})])
        .expect("start");
    assert_eq!(
        api.take_calls(),
        vec![
            Call::Bandwidth(caps::LOW_PATH_TOP_HZ * caps::BANDWIDTH_PER_RATE),
            Call::Run,
            Call::StreamStart,
            Call::StreamStop,
            Call::Stop,
            Call::Bandwidth(RSA306.bandwidth_min),
            Call::Run,
            Call::StreamStart,
        ]
    );
    device
        .apply(&DeviceSettings {
            sample_rate: Some(27_343.75),
            ..DeviceSettings::default()
        })
        .expect("step up");
    assert_eq!(
        api.take_calls(),
        vec![
            Call::StreamStop,
            Call::Stop,
            Call::Bandwidth(27_343.75 * caps::BANDWIDTH_PER_RATE),
            Call::Run,
            Call::StreamStart,
        ],
        "a buffer already sized is not sized again"
    );
    device.rx_stop();
}

#[test]
fn a_fitted_preamp_is_offered_and_switched_with_the_stream_rebuilt() {
    let api = Arc::new(FakeApi::rsa507a());
    let mut device = open(&api);
    assert!(offers(device.as_ref(), caps::PREAMP));
    device
        .rx_start(vec![RxSink::new(|_, _| {})])
        .expect("start");
    api.take_calls();
    device.apply(&preamp(true)).expect("preamp on");
    assert_eq!(
        api.take_calls(),
        vec![
            Call::StreamStop,
            Call::Stop,
            Call::Preamp(true),
            Call::Run,
            Call::StreamStart,
        ]
    );
    assert!(held(device.as_ref(), &preamp(true)));
    device.rx_stop();
}

#[test]
fn a_fitted_attenuator_starts_automatic_and_takes_a_manual_value_with_the_stream_rebuilt() {
    let api = Arc::new(FakeApi::rsa507a());
    let mut device = open(&api);
    assert!(held(device.as_ref(), &attenuation(true, 0.0)));
    device
        .rx_start(vec![RxSink::new(|_, _| {})])
        .expect("start");
    api.take_calls();
    device.apply(&attenuation(false, -20.0)).expect("manual");
    assert_eq!(
        api.take_calls(),
        vec![
            Call::StreamStop,
            Call::Stop,
            Call::AutoAttenuation(false),
            Call::Attenuation(-20.0),
            Call::Run,
            Call::StreamStart,
        ]
    );
    assert!(held(device.as_ref(), &attenuation(false, -20.0)));
    assert_eq!(device.settings().gain(caps::ATTENUATOR), Some(-20.0));
    device.rx_stop();
}

#[test]
fn an_analyser_without_an_attenuator_does_not_offer_one() {
    let api = Arc::new(FakeApi::rsa306());
    let mut device = open(&api);
    assert!(device.capabilities().stage(caps::ATTENUATOR).is_none());
    assert!(!offers(device.as_ref(), caps::AUTO_ATTENUATION));
    assert!(matches!(
        device.apply(&attenuation(false, -20.0)),
        Err(DeviceError::Unsupported(_))
    ));
}

#[test]
fn the_frequency_reference_reaches_the_analyser() {
    let api = Arc::new(FakeApi::rsa306());
    let mut device = open(&api);
    assert!(held(device.as_ref(), &source(FrequencySource::Internal)));
    api.take_calls();
    device
        .apply(&source(FrequencySource::External))
        .expect("external");
    assert_eq!(
        api.take_calls(),
        vec![Call::Source(FrequencySource::External)]
    );
    assert!(held(device.as_ref(), &source(FrequencySource::External)));
    assert!(matches!(
        device.apply(&source(FrequencySource::Gnss)),
        Err(DeviceError::Unsupported(_))
    ));
}

#[test]
fn a_reference_that_does_not_lock_fails_and_the_stream_comes_back() {
    let api = Arc::new(FakeApi::rsa306());
    api.leave_the_external_reference_open();
    let mut device = open(&api);
    device
        .rx_start(vec![RxSink::new(|_, _| {})])
        .expect("start");
    assert!(matches!(
        device.apply(&source(FrequencySource::External)),
        Err(DeviceError::Io(_))
    ));
    assert!(api.is_streaming());
    assert!(held(device.as_ref(), &source(FrequencySource::Internal)));
    device.rx_stop();
}

#[test]
fn an_analyser_without_a_preamp_does_not_offer_one() {
    let api = Arc::new(FakeApi::rsa306());
    let device = open(&api);
    assert!(!offers(device.as_ref(), caps::PREAMP));
}

#[test]
fn settings_the_analyser_lacks_are_refused() {
    let api = Arc::new(FakeApi::rsa306());
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
            sample_rate: Some(2e6),
            ..DeviceSettings::default()
        },
        DeviceSettings {
            center_hz: Some(7e9),
            ..DeviceSettings::default()
        },
        DeviceSettings {
            antenna: Some("Rx2".to_string()),
            ..DeviceSettings::default()
        },
        preamp(true),
        gain(140.0),
    ] {
        assert!(
            matches!(device.apply(&refused), Err(DeviceError::Unsupported(_))),
            "{refused:?}"
        );
    }
}

#[test]
fn samples_reach_the_sink_with_the_reference_level_at_full_scale() {
    let api = Arc::new(FakeApi::rsa306());
    let mut device = open(&api);
    device.apply(&gain(0.0)).expect("0 dBm reference");
    let (tx, rx) = mpsc::channel();
    device
        .rx_start(vec![RxSink::new(move |samples: &[Sample], _| {
            tx.send(samples.to_vec()).expect("receiver lives");
        })])
        .expect("start");
    assert!(api.is_streaming());
    api.push(1_000, tone(ZERO_DBM_VOLTS as f32, 64));
    let block = rx.recv_timeout(WAIT).expect("a block");
    assert_eq!(block.len(), 64);
    assert!((block[0].re - 1.0).abs() < 1e-5 && (block[0].im + 1.0).abs() < 1e-5);
    assert!(api.wait_until_drained(WAIT));
    device.rx_stop();
    assert!(!api.is_streaming());
}

#[test]
fn a_hole_in_the_block_timestamps_steps_the_sample_index() {
    let api = Arc::new(FakeApi::rsa306());
    let mut device = open(&api);
    let ticks = TIMESTAMP_RATE / 1_750_000;
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
    api.push(5_000, tone(0.0, 100));
    api.push(5_000 + 300 * ticks, tone(0.0, 100));
    assert_eq!(rx.recv_timeout(WAIT).expect("first"), Some((0, 100)));
    assert_eq!(rx.recv_timeout(WAIT).expect("gap"), None);
    assert_eq!(rx.recv_timeout(WAIT).expect("second"), Some((300, 100)));
    device.rx_stop();
}

#[test]
fn an_unplugged_analyser_fails_the_sink_as_disconnected() {
    let api = Arc::new(FakeApi::rsa306());
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
fn health_names_the_library_and_version_it_found() {
    let check = health(&RuntimeInfo {
        version: Some("5.6.0295.0".to_string()),
        library: Some("C:/Tektronix/RSA_API/lib/x64/RSA_API.dll".to_string()),
        error: None,
    });
    assert_eq!(check.status, CheckStatus::Ok);
    assert!(check.detail.contains("5.6.0295.0"));
    assert!(check.hint.is_none());
}

#[test]
fn a_missing_api_warns_and_says_where_to_get_it() {
    let check = health(&RuntimeInfo {
        version: None,
        library: None,
        error: Some("the Tektronix RSA API is not installed".to_string()),
    });
    assert_eq!(check.status, CheckStatus::Warn);
    assert!(
        check
            .hint
            .is_some_and(|hint| hint.contains("tek.com") && hint.contains(OVERRIDE_VAR))
    );
}

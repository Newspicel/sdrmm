#![allow(clippy::expect_used)]

use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use sdrmm_device::{DeviceDriver, RxSink, Sample, SdrDevice, lock};
use sdrmm_device_ad936x::Ad936xDriver;
use sdrmm_wire::{
    AgcSetting, DeviceSettings, ExtraValue, GainKind, GainValue, ReadoutValue, StreamSettings,
};

const RADIO: &str = "SDRMM_AD936X";

struct Radio {
    device: Box<dyn SdrDevice>,
    held: DeviceSettings,
}

impl Deref for Radio {
    type Target = Box<dyn SdrDevice>;

    fn deref(&self) -> &Self::Target {
        &self.device
    }
}

impl DerefMut for Radio {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.device
    }
}

impl Drop for Radio {
    fn drop(&mut self) {
        let held = &self.held;
        let back = DeviceSettings {
            center_hz: held.center_hz,
            sample_rate: held.sample_rate,
            bandwidth: held.bandwidth,
            rx_inputs: held.rx_inputs.clone(),
            antenna: held.antenna.clone(),
            gains: held.gains.clone(),
            agc: held.agc.clone(),
            extra: held.extra.clone(),
            streams: held
                .streams
                .iter()
                .map(|lane| StreamSettings {
                    stream: lane.stream,
                    gains: lane.gains.clone(),
                    agc: lane.agc.clone(),
                    ..StreamSettings::default()
                })
                .collect(),
            ..DeviceSettings::default()
        };
        self.device.rx_stop();
        if let Err(e) = self.device.apply(&back) {
            eprintln!("the radio did not take back what it held: {e}");
        }
    }
}

fn open() -> Radio {
    let address = std::env::var(RADIO).expect("set SDRMM_AD936X to the radio's address");
    let driver = Ad936xDriver::searching([]);
    let info = driver.resolve(&address).expect("an addressable radio");
    let device = driver.open(&info).expect("opens");
    Radio {
        held: device.settings().clone(),
        device,
    }
}

#[test]
#[ignore = "needs an AD936x radio at SDRMM_AD936X"]
fn a_real_radio_holds_every_setting_it_is_given() {
    let mut device = open();
    let lanes = device.capabilities().rx_streams;

    device
        .apply(&DeviceSettings {
            center_hz: Some(100e6),
            gains: vec![GainValue::new(GainKind::Tuner, 73.0)],
            agc: Some(AgcSetting::off()),
            ..DeviceSettings::default()
        })
        .expect("the low band");
    assert_eq!(device.capabilities().gains[0].range.max, 73.0);
    assert_eq!(device.settings().gains[0].value_db, 73.0);

    device
        .apply(&DeviceSettings {
            center_hz: Some(5.8e9),
            gains: vec![GainValue::new(GainKind::Tuner, 73.0)],
            ..DeviceSettings::default()
        })
        .expect("a gain past the high band's reach is clamped");
    assert_eq!(device.capabilities().gains[0].range.max, 62.0);

    let antennas = device.capabilities().antennas.clone();
    for antenna in &antennas {
        device
            .apply(&DeviceSettings {
                antenna: Some(antenna.clone()),
                ..DeviceSettings::default()
            })
            .expect("every offered input is one the radio takes");
        assert_eq!(device.settings().antenna.as_ref(), Some(antenna));
    }

    if lanes > 1 {
        device
            .apply(&DeviceSettings {
                streams: vec![StreamSettings {
                    stream: 1,
                    agc: Some(AgcSetting::in_mode(true, "slow_attack")),
                    ..StreamSettings::default()
                }],
                ..DeviceSettings::default()
            })
            .expect("a lane's own gain loop");
        let gains = device.agc_gains().expect("read back");
        assert_eq!(gains.iter().map(|g| g.stream).collect::<Vec<_>>(), [1]);
    }
}

#[test]
#[ignore = "needs an AD936x radio at SDRMM_AD936X"]
fn a_real_radio_streams_every_lane_at_a_decimated_rate() {
    let mut device = open();
    let rate = 1e6;
    device
        .apply(&DeviceSettings {
            sample_rate: Some(rate),
            ..DeviceSettings::default()
        })
        .expect("a rate below the transceiver");
    let taken = device.settings().sample_rate.expect("a rate");
    assert!((taken - rate).abs() < 2.0, "{taken}");

    let lanes = device.capabilities().rx_streams as usize;
    let counts: Vec<Arc<Mutex<(usize, f32, u64)>>> = (0..lanes).map(|_| Arc::default()).collect();
    let sinks = counts
        .iter()
        .map(|count| {
            let count = count.clone();
            RxSink::new(move |samples: &[Sample], index| {
                let mut count = lock(&count);
                count.0 += samples.len();
                count.2 = index + samples.len() as u64;
                count.1 = samples.iter().fold(count.1, |peak, s| peak.max(s.norm()));
            })
        })
        .collect();
    let started = Instant::now();
    device.rx_start(sinks).expect("streams");
    std::thread::sleep(Duration::from_secs(2));
    device.rx_stop();
    let span = started.elapsed().as_secs_f64();

    for (lane, count) in counts.iter().enumerate() {
        let (samples, peak, reached) = *lock(count);
        assert_eq!(
            reached, samples as u64,
            "lane {lane} reported a loss it did not have"
        );
        let achieved = samples as f64 / span;
        assert!(
            achieved > rate * 0.8 && achieved < rate * 1.1,
            "lane {lane}: {achieved:.0} S/s"
        );
        assert!(peak > 0.0, "lane {lane} carried only zeros");
    }
}

#[test]
#[ignore = "needs an AD936x radio at SDRMM_AD936X"]
fn a_real_radio_faster_than_its_link_reports_the_samples_it_lost() {
    let mut device = open();
    let fastest = device.capabilities().sample_rate_ranges[0].max;
    device
        .apply(&DeviceSettings {
            sample_rate: Some(fastest),
            ..DeviceSettings::default()
        })
        .expect("the fastest rate");
    let lanes = device.capabilities().rx_streams as usize;
    let seen: Vec<Arc<Mutex<(u64, u64)>>> = (0..lanes).map(|_| Arc::default()).collect();
    let sinks = seen
        .iter()
        .map(|seen| {
            let seen = seen.clone();
            RxSink::new(move |samples: &[Sample], index| {
                let mut seen = lock(&seen);
                seen.0 += samples.len() as u64;
                seen.1 = index + samples.len() as u64;
            })
        })
        .collect();
    device.rx_start(sinks).expect("streams");
    std::thread::sleep(Duration::from_secs(3));
    device.rx_stop();

    for (lane, seen) in seen.iter().enumerate() {
        let (delivered, reached) = *lock(seen);
        assert!(
            reached > delivered + delivered / 2,
            "lane {lane}: {delivered} delivered, timeline at {reached}"
        );
    }
}

#[test]
#[ignore = "needs an AD936x radio at SDRMM_AD936X"]
fn a_real_radio_gives_one_lane_the_whole_link() {
    let mut device = open();
    if device.capabilities().rx_inputs.is_empty() {
        return;
    }
    let rate = 15e6;
    device
        .apply(&DeviceSettings {
            rx_inputs: Some(vec![0]),
            sample_rate: Some(rate),
            ..DeviceSettings::default()
        })
        .expect("one lane at the link's full rate");
    let seen: Arc<Mutex<(u64, u64)>> = Arc::default();
    let counted = seen.clone();
    device
        .rx_start(vec![RxSink::new(move |samples: &[Sample], index| {
            let mut seen = lock(&counted);
            seen.0 += samples.len() as u64;
            seen.1 = index + samples.len() as u64;
        })])
        .expect("streams");
    std::thread::sleep(Duration::from_secs(3));
    device.rx_stop();
    let (delivered, reached) = *lock(&seen);
    assert_eq!(delivered, reached, "one lane at {rate} S/s lost samples");
    assert!(delivered as f64 > rate * 2.5, "{delivered}");
}

fn extra(name: &str, value: serde_json::Value) -> ExtraValue {
    ExtraValue {
        name: name.to_string(),
        value,
    }
}

fn held(device: &dyn SdrDevice, name: &str) -> Option<serde_json::Value> {
    device
        .settings()
        .extra
        .iter()
        .find(|extra| extra.name == name)
        .map(|extra| extra.value.clone())
}

fn reopened() -> Box<dyn SdrDevice> {
    let address = std::env::var(RADIO).expect("set SDRMM_AD936X to the radio's address");
    let driver = Ad936xDriver::searching([]);
    let info = driver.resolve(&address).expect("an addressable radio");
    driver.open(&info).expect("opens a second time")
}

fn capture(device: &mut Radio, samples: usize) -> Vec<Sample> {
    let lanes = device.capabilities().rx_streams as usize;
    let taken: Arc<Mutex<Vec<Sample>>> = Arc::default();
    let filled = taken.clone();
    let mut sinks = vec![RxSink::new(move |block: &[Sample], _| {
        let mut taken = lock(&filled);
        let room = samples.saturating_sub(taken.len());
        taken.extend_from_slice(&block[..room.min(block.len())]);
    })];
    sinks.extend((1..lanes).map(|_| RxSink::new(|_: &[Sample], _| {})));
    device.rx_start(sinks).expect("streams");
    let started = Instant::now();
    while lock(&taken).len() < samples && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    device.rx_stop();
    let taken = lock(&taken).clone();
    assert_eq!(taken.len(), samples, "the capture filled");
    taken
}

fn power_at(samples: &[Sample], hz: f64, rate: f64) -> f64 {
    let step = -std::f64::consts::TAU * hz / rate;
    let (re, im) = samples
        .iter()
        .enumerate()
        .fold((0.0, 0.0), |(re, im), (n, sample)| {
            let (sin, cos) = (step * n as f64).sin_cos();
            let (i, q) = (f64::from(sample.re), f64::from(sample.im));
            (re + i * cos - q * sin, im + i * sin + q * cos)
        });
    10.0 * ((re * re + im * im) / (samples.len() as f64).powi(2)).log10()
}

#[test]
#[ignore = "needs an AD936x radio at SDRMM_AD936X"]
fn a_real_radio_reads_out_its_temperature_and_signal_level() {
    let device = open();
    let readouts = device.readouts().expect("readouts");
    let number = |name: &str| {
        readouts
            .iter()
            .filter(|readout| readout.name == name)
            .map(|readout| match readout.value {
                ReadoutValue::Number { value, .. } => value,
                ReadoutValue::Flag { .. } => panic!("{name} is a number"),
            })
            .collect::<Vec<f64>>()
    };
    let chip = number("temperature");
    assert!(
        chip.len() == 1 && (5.0..110.0).contains(&chip[0]),
        "{readouts:?}"
    );
    assert_eq!(
        number("rssi").len(),
        device.capabilities().rx_streams as usize,
        "{readouts:?}"
    );
    assert!(number("rssi").iter().all(|db| *db < 0.0), "{readouts:?}");
}

#[test]
#[ignore = "needs an AD936x radio at SDRMM_AD936X"]
fn a_real_radio_holds_its_chip_settings() {
    let mut device = open();
    let names: Vec<String> = device
        .capabilities()
        .extra
        .iter()
        .map(|setting| setting.name().to_string())
        .collect();
    for name in [
        "rate_governor",
        "calibration",
        "ensm",
        "agc_high",
        "agc_low",
        "rx_lo_external",
        "tone_1",
    ] {
        assert!(
            names.iter().any(|offered| offered == name),
            "{name} in {names:?}"
        );
    }
    let wanted = [
        extra("rate_governor", serde_json::json!("highest_osr")),
        extra("calibration", serde_json::json!("manual")),
        extra("agc_high", serde_json::json!(-12.0)),
        extra("agc_low", serde_json::json!(-15.0)),
        extra("ensm", serde_json::json!("fdd")),
    ];
    device
        .apply(&DeviceSettings {
            extra: wanted.to_vec(),
            ..DeviceSettings::default()
        })
        .expect("the chip takes them");
    let fresh = reopened();
    for want in &wanted {
        assert_eq!(
            held(fresh.as_ref(), &want.name).as_ref(),
            Some(&want.value),
            "{} reads back from the radio",
            want.name
        );
    }
    assert!(
        device
            .apply(&DeviceSettings {
                extra: vec![extra("agc_low", serde_json::json!(-10.0))],
                ..DeviceSettings::default()
            })
            .is_err(),
        "a low threshold above the high one is refused"
    );
}

#[test]
#[ignore = "needs an AD936x radio at SDRMM_AD936X"]
fn a_real_radio_hears_its_own_test_tone_across_recalled_retunes() {
    let mut device = open();
    if device.capabilities().tx_streams == 0 {
        return;
    }
    let rate = 4e6;
    let offset = 200e3;
    let home = 869_500_000.0;
    device
        .apply(&DeviceSettings {
            center_hz: Some(home),
            sample_rate: Some(rate),
            agc: Some(AgcSetting::off()),
            gains: vec![
                GainValue::new(GainKind::Tuner, 50.0),
                GainValue::new(GainKind::Tx, -30.0),
            ],
            extra: vec![
                extra("tone_1", serde_json::json!(true)),
                extra("tone_1_hz", serde_json::json!(offset)),
                extra("tone_1_dbfs", serde_json::json!(-6.0)),
                extra("tone_2", serde_json::json!(false)),
            ],
            ..DeviceSettings::default()
        })
        .expect("a tone");
    let mut retunes = Vec::new();
    for center in [home, home + 1e6, home, home + 1e6, home] {
        let started = Instant::now();
        device
            .apply(&DeviceSettings {
                center_hz: Some(center),
                ..DeviceSettings::default()
            })
            .expect("retunes");
        retunes.push(started.elapsed());
        let samples = capture(&mut device, 65_536);
        let tone = power_at(&samples, offset, rate);
        let mirror = power_at(&samples, -offset, rate);
        let elsewhere = power_at(&samples, offset + 377e3, rate);
        assert!(
            tone > mirror + 20.0 && tone > elsewhere + 20.0,
            "at {center}: tone {tone:.1} dB, mirror {mirror:.1}, elsewhere {elsewhere:.1}"
        );
    }
    eprintln!("retunes took {retunes:?}");

    device
        .apply(&DeviceSettings {
            extra: vec![extra("tone_1", serde_json::json!(false))],
            ..DeviceSettings::default()
        })
        .expect("silence");
    let samples = capture(&mut device, 65_536);
    let quiet = power_at(&samples, offset, rate);
    let elsewhere = power_at(&samples, offset + 377e3, rate);
    assert!(
        quiet < elsewhere + 10.0,
        "tone off: {quiet:.1} dB vs {elsewhere:.1}"
    );
}

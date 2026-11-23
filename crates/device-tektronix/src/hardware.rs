use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use sdrmm_device::{LaneEvent, SinkItem};

use super::*;

const TUNE_HZ: f64 = 100e6;
const RETUNE_HZ: f64 = 433.92e6;
const FM_BAND_HZ: f64 = 98e6;
const FM_BAND_RATE_HZ: f64 = 28e6;
const BASE_GAIN_DB: f64 = 20.0;
const GAIN_STEP_DB: f64 = 10.0;
const LEVEL_TOLERANCE_DB: f64 = 3.0;
const SETTLE: Duration = Duration::from_millis(500);
const LISTEN: Duration = Duration::from_secs(2);
const DELIVERED_AT_LEAST: f64 = 0.9;

#[derive(Default)]
struct Heard {
    samples: AtomicU64,
    gaps: AtomicU64,
    power_bits: AtomicU64,
}

impl Heard {
    fn sink(self: &Arc<Self>) -> RxSink {
        let heard = self.clone();
        RxSink::with_items(
            move |item| match item {
                SinkItem::Samples { samples, .. } => {
                    let power = samples.iter().map(|s| f64::from(s.norm_sqr())).sum::<f64>()
                        / samples.len().max(1) as f64;
                    heard.power_bits.store(power.to_bits(), Ordering::Release);
                    heard
                        .samples
                        .fetch_add(samples.len() as u64, Ordering::AcqRel);
                }
                SinkItem::Event(LaneEvent::Uncertain { .. }) => {
                    heard.gaps.fetch_add(1, Ordering::AcqRel);
                }
                SinkItem::Event(_) => {}
            },
            |error| panic!("the stream failed: {error}"),
        )
    }

    fn samples(&self) -> u64 {
        self.samples.load(Ordering::Acquire)
    }

    fn gaps(&self) -> u64 {
        self.gaps.load(Ordering::Acquire)
    }

    fn level_dbfs(&self) -> f64 {
        10.0 * f64::from_bits(self.power_bits.load(Ordering::Acquire)).log10()
    }
}

fn open_through_the_driver() -> Box<dyn SdrDevice> {
    let driver = TektronixDriver::new();
    let found = driver.probe();
    assert_eq!(found.len(), 1, "attach exactly one analyser: {found:?}");
    driver.open(&found[0]).expect("open the analyser")
}

fn tuned(center_hz: f64) -> DeviceSettings {
    DeviceSettings {
        center_hz: Some(center_hz),
        ..DeviceSettings::default()
    }
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

/// Samples and gaps heard over [`LISTEN`], once the stream has settled.
fn listen(heard: &Heard) -> (u64, u64, Duration) {
    std::thread::sleep(SETTLE);
    let (samples, gaps, started) = (heard.samples(), heard.gaps(), Instant::now());
    std::thread::sleep(LISTEN);
    (
        heard.samples() - samples,
        heard.gaps() - gaps,
        started.elapsed(),
    )
}

#[test]
#[ignore = "requires one idle Tektronix RSA and its RSA API"]
fn connected_analyser_is_listed_with_its_serial_and_limits() {
    let device = open_through_the_driver();
    let caps = device.capabilities();
    println!(
        "tunes {} to {} Hz at {:?} S/s",
        caps.freq_ranges[0].min, caps.freq_ranges[0].max, caps.sample_rates
    );
    assert!(!caps.sample_rates.is_empty());
    assert!(caps.freq_ranges[0].max > caps.freq_ranges[0].min);
}

#[test]
#[ignore = "requires one idle Tektronix RSA and its RSA API"]
fn connected_analyser_delivers_every_rate_without_loss() {
    let mut device = open_through_the_driver();
    device.apply(&tuned(TUNE_HZ)).expect("tune");
    for rate in device.capabilities().sample_rates.clone() {
        device
            .apply(&DeviceSettings {
                sample_rate: Some(rate),
                ..DeviceSettings::default()
            })
            .expect("rate");
        assert_eq!(device.settings().sample_rate, Some(rate));
        let heard = Arc::new(Heard::default());
        device.rx_start(vec![heard.sink()]).expect("start");
        let (samples, gaps, elapsed) = listen(&heard);
        device.rx_stop();
        let wanted = rate * elapsed.as_secs_f64();
        println!("{rate} S/s: {samples} of {wanted:.0} samples, {gaps} gaps");
        assert_eq!(gaps, 0, "{rate} S/s lost samples");
        assert!(
            samples as f64 >= wanted * DELIVERED_AT_LEAST,
            "{rate} S/s delivered {samples} of {wanted:.0}"
        );
    }
}

#[test]
#[ignore = "requires one idle Tektronix RSA and its RSA API"]
fn connected_analyser_retunes_and_changes_rate_while_streaming() {
    let mut device = open_through_the_driver();
    device.apply(&tuned(TUNE_HZ)).expect("tune");
    let heard = Arc::new(Heard::default());
    device.rx_start(vec![heard.sink()]).expect("start");
    std::thread::sleep(SETTLE);
    device.apply(&tuned(RETUNE_HZ)).expect("retune");
    assert_eq!(device.settings().center_hz, Some(RETUNE_HZ));
    let (samples, _, _) = listen(&heard);
    assert!(samples > 0, "the stream stopped after a retune");
    assert_eq!(heard.gaps(), 1, "the retune pause is booked as one gap");
    device
        .apply(&DeviceSettings {
            sample_rate: Some(14e6),
            ..DeviceSettings::default()
        })
        .expect("rate");
    let (samples, _, elapsed) = listen(&heard);
    device.rx_stop();
    assert!(
        samples as f64 >= 14e6 * elapsed.as_secs_f64() * DELIVERED_AT_LEAST,
        "the stream did not come back at the new rate: {samples}"
    );
}

#[test]
#[ignore = "requires one idle Tektronix RSA, its RSA API and FM broadcast on its input"]
fn connected_analyser_gain_moves_the_level_by_as_much_while_streaming() {
    let mut device = open_through_the_driver();
    device
        .apply(&DeviceSettings {
            sample_rate: Some(FM_BAND_RATE_HZ),
            ..tuned(FM_BAND_HZ)
        })
        .expect("tune");
    let heard = Arc::new(Heard::default());
    device.rx_start(vec![heard.sink()]).expect("start");
    let mut levels = Vec::new();
    for step in 0..3 {
        device
            .apply(&gain(BASE_GAIN_DB + f64::from(step) * GAIN_STEP_DB))
            .expect("gain");
        std::thread::sleep(SETTLE);
        levels.push(heard.level_dbfs());
    }
    device.rx_stop();
    println!("levels in {GAIN_STEP_DB} dB gain steps: {levels:?} dBFS");
    for pair in levels.windows(2) {
        assert!(
            (pair[1] - pair[0] - GAIN_STEP_DB).abs() < LEVEL_TOLERANCE_DB,
            "a {GAIN_STEP_DB} dB gain step moved the level from {} to {} dBFS",
            pair[0],
            pair[1]
        );
    }
}

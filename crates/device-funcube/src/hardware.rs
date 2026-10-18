use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use sdrmm_device::{CaptureStream, Next, Sample, SampleConverter};
use sdrmm_dsp::fft::FftPair;
use sdrmm_wire::{BandwidthSetting, ExtraValue, GainKind, GainValue};

use super::*;
use crate::{
    caps::{LNA, MIXER, RF_FILTER},
    protocol::{IfFilter, RfFilter},
};

const SETTLE: Duration = Duration::from_millis(300);
const FFT_LEN: usize = 4096;
const SPECTRUM_SAMPLES: usize = FFT_LEN * 24;
const SHIFT_HZ: u32 = 40_000;

fn seconds() -> u64 {
    std::env::var("SDRMM_FUNCUBE_TEST_SECONDS")
        .map(|value| value.parse().expect("duration"))
        .unwrap_or(5)
}

fn only_funcube() -> String {
    let found = discovery::list().expect("list USB devices");
    assert_eq!(found.len(), 1, "attach exactly one FUNcube Dongle Pro+");
    discovery::key(&found[0])
}

fn open_both() -> (FunCube<platform::Control>, platform::Port) {
    let (control, port) = platform::open(&only_funcube()).expect("open the dongle");
    (FunCube::open(control).expect("open FUNcube"), port)
}

fn open_raw() -> FunCube<platform::Control> {
    open_both().0
}

fn capture(port: &platform::Port, samples: usize) -> Vec<Sample> {
    let stream = port.start().expect("start stream");
    let mut converter = FunCubeConverter::new(platform::FORMAT, platform::BLOCK_SAMPLES);
    let started = Instant::now();
    let mut out = Vec::with_capacity(samples);
    while out.len() < samples {
        match stream.next_block(Duration::from_secs(1)) {
            Next::Block(block) => {
                let iq = converter.convert(&block);
                if started.elapsed() >= SETTLE {
                    out.extend_from_slice(iq);
                }
            }
            Next::Idle => panic!("no block within a second"),
            Next::Ended => panic!("stream ended: {:?}", stream.failure()),
        }
    }
    out.truncate(samples);
    out
}

fn spectrum(iq: &[Sample]) -> Vec<f64> {
    let mut fft = FftPair::new(FFT_LEN);
    let window: Vec<f32> = (0..FFT_LEN)
        .map(|n| 0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / FFT_LEN as f32).cos())
        .collect();
    let mut power = vec![0.0f64; FFT_LEN];
    for frame in iq.as_chunks::<FFT_LEN>().0 {
        let mut buf: Vec<Sample> = frame.iter().zip(&window).map(|(s, w)| s * w).collect();
        fft.forward(&mut buf);
        for (bin, value) in power.iter_mut().zip(&buf) {
            *bin += f64::from(value.norm_sqr());
        }
    }
    (0..FFT_LEN)
        .map(|bin| power[(bin + FFT_LEN / 2) % FFT_LEN])
        .collect()
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[sorted.len() / 2]
}

fn db(ratio: f64) -> f64 {
    10.0 * ratio.log10()
}

fn floor_db(port: &platform::Port) -> f64 {
    db(median(&spectrum(&capture(port, SPECTRUM_SAMPLES))))
}

fn bin_hz() -> f64 {
    f64::from(SAMPLE_RATE_HZ) / FFT_LEN as f64
}

fn shape(spectrum: &[f64]) -> Vec<f64> {
    let reach = (80_000.0 / bin_hz()) as usize;
    let centre = FFT_LEN / 2;
    let logs: Vec<f64> = spectrum[centre - reach..=centre + reach]
        .iter()
        .map(|power| db(*power))
        .collect();
    let mean = logs.iter().sum::<f64>() / logs.len() as f64;
    logs.iter().map(|value| value - mean).collect()
}

fn agreement(before: &[f64], after: &[f64], lag: isize) -> f64 {
    before
        .iter()
        .enumerate()
        .filter_map(|(bin, value)| {
            let moved = usize::try_from(bin as isize + lag).ok()?;
            after.get(moved).map(|other| value * other)
        })
        .sum()
}

#[test]
#[ignore = "requires one idle FUNcube Dongle Pro+"]
fn connected_funcube_holds_every_write_it_is_given() {
    let mut radio = open_raw();
    assert!(
        radio.firmware().starts_with("FCDAPP"),
        "{}",
        radio.firmware()
    );
    radio.set_frequency_hz(145_800_000).expect("tune");
    radio.set_lna(false).expect("lna");
    radio.set_mixer(false).expect("mixer");
    radio.set_if_gain_db(20).expect("if gain");
    radio.set_if_filter(IfFilter::Khz600).expect("if filter");
    radio
        .set_rf_filter(Some(RfFilter::Mhz435))
        .expect("rf filter");
    assert_eq!(radio.read_back().expect("read back"), *radio.config());
    assert_eq!(radio.active_rf_filter().expect("rf"), RfFilter::Mhz435);
    radio.set_rf_filter(None).expect("auto");
    let chosen = radio.active_rf_filter().expect("rf");
    eprintln!("auto filter at 145.8 MHz: {chosen:?}");
    assert!(matches!(chosen, RfFilter::Mhz125To250 | RfFilter::Mhz145));
}

#[test]
#[ignore = "requires one idle FUNcube Dongle Pro+"]
fn connected_funcube_ppm_moves_the_tuning_by_the_correction() {
    let mut radio = open_raw();
    radio.set_frequency_hz(100_000_000).expect("tune");
    radio.set_ppm(10.0).expect("ppm");
    assert_eq!(radio.read_back().expect("read back").tuned_hz, 99_999_000);
    radio.set_ppm(0.0).expect("ppm");
    assert_eq!(radio.read_back().expect("read back").tuned_hz, 100_000_000);
}

#[test]
#[ignore = "requires one idle FUNcube Dongle Pro+"]
fn connected_funcube_reopens_at_once_after_closing() {
    for _ in 0..3 {
        let started = Instant::now();
        let radio = open_raw();
        drop(radio);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}

#[test]
#[ignore = "requires one idle FUNcube Dongle Pro+"]
fn connected_funcube_gain_stages_move_the_level() {
    let (mut radio, port) = open_both();
    radio.set_frequency_hz(435_000_000).expect("tune");
    radio.set_lna(false).expect("lna");
    radio.set_mixer(false).expect("mixer");
    let bare = floor_db(&port);
    radio.set_mixer(true).expect("mixer");
    let mixer = floor_db(&port);
    radio.set_lna(true).expect("lna");
    let both = floor_db(&port);
    radio.set_if_gain_db(10).expect("if gain");
    let if_gain = floor_db(&port);
    radio.set_if_gain_db(0).expect("if gain");
    eprintln!(
        "floor: bare {bare:.1} mixer {:+.1} lna {:+.1} if10 {:+.1} dB",
        mixer - bare,
        both - mixer,
        if_gain - both
    );
    assert!(mixer - bare > 3.0, "the mixer gain should lift the floor");
    assert!(both - mixer > 1.0, "the LNA should lift the floor");
    assert!(
        if_gain - both > 6.0,
        "10 dB of IF gain should lift the floor"
    );
}

#[test]
#[ignore = "requires one FUNcube Dongle Pro+ with an antenna in reach of FM broadcast"]
fn connected_funcube_spectrum_is_upright() {
    let (mut radio, port) = open_both();
    let mut best = (0.0f64, 0.0f64, 0);
    for mhz in (88..=107).step_by(1) {
        let hz = mhz * 1_000_000;
        radio.set_frequency_hz(hz).expect("tune");
        let before = shape(&spectrum(&capture(&port, SPECTRUM_SAMPLES)));
        radio.set_frequency_hz(hz + SHIFT_HZ).expect("tune");
        let after = shape(&spectrum(&capture(&port, SPECTRUM_SAMPLES)));
        let lag = (f64::from(SHIFT_HZ) / bin_hz()).round() as isize;
        let upright = agreement(&before, &after, -lag);
        let inverted = agreement(&before, &after, lag);
        eprintln!("{mhz} MHz: upright {upright:.0} inverted {inverted:.0}");
        if (upright - inverted).abs() > (best.0 - best.1).abs() {
            best = (upright, inverted, mhz);
        }
    }
    let (upright, inverted, mhz) = best;
    assert!(
        upright > inverted,
        "at {mhz} MHz the spectrum moved the wrong way when the dongle tuned up"
    );
}

#[test]
#[ignore = "requires one idle FUNcube Dongle Pro+; set SDRMM_FUNCUBE_BIAS_TEE=1 with no DC-shorted antenna"]
fn connected_funcube_bias_tee_switches() {
    if std::env::var("SDRMM_FUNCUBE_BIAS_TEE").as_deref() != Ok("1") {
        eprintln!("skipped: SDRMM_FUNCUBE_BIAS_TEE is not 1");
        return;
    }
    let mut radio = open_raw();
    radio.set_bias_tee(true).expect("bias tee on");
    assert!(radio.read_back().expect("read back").bias_tee);
    radio.set_bias_tee(false).expect("bias tee off");
    assert!(!radio.read_back().expect("read back").bias_tee);
}

#[derive(Default)]
struct Seen {
    samples: u64,
    next_index: u64,
    index_gaps: u64,
    non_finite: u64,
    first: Option<Instant>,
    last: Option<Instant>,
    after_first: u64,
}

type Failure = Arc<Mutex<Option<DeviceError>>>;

fn watching_sink() -> (RxSink, Arc<Mutex<Seen>>, Failure) {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let failure = Arc::new(Mutex::new(None));
    let sink = {
        let seen = seen.clone();
        let failure = failure.clone();
        RxSink::with_fatal_handler(
            move |samples: &[Sample], index: u64| {
                let mut seen = lock(&seen);
                if index != seen.next_index {
                    seen.index_gaps += 1;
                }
                seen.next_index = index + samples.len() as u64;
                seen.non_finite += samples
                    .iter()
                    .filter(|s| !s.re.is_finite() || !s.im.is_finite())
                    .count() as u64;
                let now = Instant::now();
                if seen.first.is_some() {
                    seen.after_first += samples.len() as u64;
                } else {
                    seen.first = Some(now);
                }
                seen.last = Some(now);
                seen.samples += samples.len() as u64;
            },
            move |err| *lock(&failure) = Some(err),
        )
    };
    (sink, seen, failure)
}

fn open_through_the_driver() -> Box<dyn SdrDevice> {
    let driver = FunCubeDriver::new();
    let found = driver.probe();
    assert_eq!(found.len(), 1, "attach exactly one FUNcube Dongle Pro+");
    driver.open(&found[0]).expect("open through the driver")
}

#[test]
#[ignore = "requires one idle FUNcube Dongle Pro+"]
fn connected_funcube_delivers_through_the_device_api() {
    let mut device = open_through_the_driver();
    device
        .apply(&DeviceSettings {
            center_hz: Some(433_920_000.0),
            gains: vec![
                GainValue {
                    stage: LNA.to_string(),
                    value_db: 0.0,
                },
                GainValue {
                    stage: MIXER.to_string(),
                    value_db: caps::ON,
                },
                GainValue::new(GainKind::If, 12.0),
            ],
            bandwidth: Some(BandwidthSetting::Manual { hz: 300e3 }),
            extra: vec![ExtraValue {
                name: RF_FILTER.to_string(),
                value: "410_875".into(),
            }],
            ..DeviceSettings::default()
        })
        .expect("apply");
    let settings = device.settings();
    assert_eq!(settings.center_hz, Some(433_920_000.0));
    assert_eq!(settings.gain(LNA), Some(0.0));
    assert_eq!(settings.gain(MIXER), Some(caps::ON));
    assert_eq!(settings.gain(GainKind::If.name()), Some(12.0));
    assert_eq!(
        settings.bandwidth,
        Some(BandwidthSetting::Manual { hz: 300e3 })
    );

    for round in 0..2 {
        let (sink, seen, failure) = watching_sink();
        device.rx_start(vec![sink]).expect("rx start");
        std::thread::sleep(Duration::from_secs(seconds()));
        device.rx_stop();
        let seen = lock(&seen);
        let failure = lock(&failure);
        let elapsed = seen
            .last
            .zip(seen.first)
            .map(|(last, first)| last.duration_since(first).as_secs_f64())
            .expect("samples arrived");
        let rate = seen.after_first as f64 / elapsed;
        eprintln!(
            "round {round}: rate {rate:.0} samples {} gaps {} non-finite {} failure {failure:?}",
            seen.samples, seen.index_gaps, seen.non_finite
        );
        assert!(failure.is_none(), "stream failed: {failure:?}");
        assert_eq!(seen.index_gaps, 0, "samples went missing");
        assert_eq!(seen.non_finite, 0);
        assert!(
            (rate / f64::from(SAMPLE_RATE_HZ) - 1.0).abs() < 0.02,
            "rate {rate}"
        );
    }
}

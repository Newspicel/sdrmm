use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use arc_swap::ArcSwap;
use num_complex::Complex;
use sdrmm_device::{DeviceError, RxSink, SdrDevice, Worker, check_stream_settings, single_rx_sink};
use sdrmm_modem::synth::{self, Catalog, Signal};
use sdrmm_wire::{
    Agc, ArgumentOption, Capabilities, Coherence, DcArtifact, DeviceSettings, Duplex, ExtraSetting,
    ExtraValue, NoiseSource, Range, StreamScope,
};

pub const SIGNAL_SETTING: &str = "signal";
pub const OFFSET_SETTING: &str = "offset_hz";
pub const LEVEL_SETTING: &str = "level_db";
pub const NOISE_SETTING: &str = "noise_db";
pub const GAP_SETTING: &str = "gap_s";

pub const MAX_GENERATORS: usize = 64;
pub const NOISE_OFF_DB: f64 = -140.0;
const MIN_LEVEL_DB: f64 = -60.0;

const BLOCK_SECS: f64 = 0.025;
const DEFAULT_LEVEL_DB: f64 = -12.0;
const DEFAULT_NOISE_DB: f64 = -60.0;
const MAX_OFFSET_HZ: f64 = 10_000_000.0;
const MAX_GAP_S: f64 = 10.0;
const GAP_STEP_S: f64 = 0.1;

#[must_use]
pub fn options(catalog: &Catalog) -> Vec<ArgumentOption> {
    catalog
        .signals
        .iter()
        .map(|signal| ArgumentOption {
            value: signal.id.to_owned(),
            label: Some(signal.label.to_owned()),
        })
        .collect()
}

#[must_use]
pub fn capabilities(catalog: &Catalog) -> Capabilities {
    Capabilities {
        freq_ranges: vec![Range {
            min: 0.0,
            max: 6_000_000_000.0,
            step: None,
        }],
        sample_rates: catalog.rates(),
        sample_rate_ranges: Vec::new(),
        gains: Vec::new(),
        antennas: Vec::new(),
        bandwidths: Vec::new(),
        bandwidth_ranges: Vec::new(),
        bandwidth_auto: false,
        bias_tee: false,
        agc: Agc::None,
        extra: vec![
            ExtraSetting::choice(
                SIGNAL_SETTING,
                "Signal",
                options(catalog),
                catalog.default_id,
            ),
            ExtraSetting::range(
                OFFSET_SETTING,
                "Offset",
                Range {
                    min: -MAX_OFFSET_HZ,
                    max: MAX_OFFSET_HZ,
                    step: None,
                },
                "Hz",
            ),
            ExtraSetting::range(
                LEVEL_SETTING,
                "Level",
                Range {
                    min: MIN_LEVEL_DB,
                    max: 0.0,
                    step: Some(1.0),
                },
                "dB",
            ),
            ExtraSetting::range(
                NOISE_SETTING,
                "Noise",
                Range {
                    min: NOISE_OFF_DB,
                    max: 0.0,
                    step: Some(1.0),
                },
                "dB",
            ),
            ExtraSetting::range(
                GAP_SETTING,
                "Gap",
                Range {
                    min: 0.0,
                    max: MAX_GAP_S,
                    step: Some(GAP_STEP_S),
                },
                "s",
            ),
        ],
        ppm: false,
        duplex: Duplex::RxOnly,
        rx_streams: 1,
        tx_streams: 0,
        per_stream: StreamScope::default(),
        directional: None,
        dc_artifact: DcArtifact::None,
        hardware_sweep: false,
        coherence: Coherence::None,
        noise_source: NoiseSource::None,
        retune_keeps_phase: false,
        rx_inputs: Vec::new(),
        clean_fraction: None,
        bank: None,
    }
}

#[derive(Clone, Copy)]
struct Params {
    catalog: &'static Catalog,
    signal: &'static Signal,
    rate_hz: f64,
    offset_hz: f64,
    level_db: f64,
    noise_db: f64,
    gap_s: f64,
}

impl Params {
    fn new(catalog: &'static Catalog, signal: &'static Signal) -> Self {
        Self {
            catalog,
            signal,
            rate_hz: signal.rate_hz,
            offset_hz: 0.0,
            level_db: DEFAULT_LEVEL_DB,
            noise_db: DEFAULT_NOISE_DB,
            gap_s: 0.5,
        }
    }

    fn renders_the_same_as(&self, other: &Self) -> bool {
        self.signal.id == other.signal.id
            && self.rate_hz.to_bits() == other.rate_hz.to_bits()
            && self.offset_hz.to_bits() == other.offset_hz.to_bits()
            && self.level_db.to_bits() == other.level_db.to_bits()
            && self.noise_db.to_bits() == other.noise_db.to_bits()
    }
}

pub struct SignalGen {
    capabilities: Capabilities,
    settings: DeviceSettings,
    params: Params,
    shared: Arc<ArcSwap<Params>>,
    worker: Worker,
}

impl SignalGen {
    pub fn new(catalog: &'static Catalog) -> Result<Self, DeviceError> {
        let signal = catalog
            .default_signal()
            .ok_or_else(|| DeviceError::NotFound("no signal catalog is loaded".to_string()))?;
        let params = Params::new(catalog, signal);
        Ok(Self {
            capabilities: capabilities(catalog),
            settings: settings_of(&params),
            params,
            shared: Arc::new(ArcSwap::from_pointee(params)),
            worker: Worker::new(),
        })
    }
}

fn settings_of(params: &Params) -> DeviceSettings {
    DeviceSettings {
        center_hz: Some(params.catalog.center_hz),
        sample_rate: Some(params.rate_hz),
        extra: vec![
            ExtraValue {
                name: SIGNAL_SETTING.to_string(),
                value: serde_json::Value::String(params.signal.id.to_string()),
            },
            ExtraValue {
                name: OFFSET_SETTING.to_string(),
                value: number(params.offset_hz),
            },
            ExtraValue {
                name: LEVEL_SETTING.to_string(),
                value: number(params.level_db),
            },
            ExtraValue {
                name: NOISE_SETTING.to_string(),
                value: number(params.noise_db),
            },
            ExtraValue {
                name: GAP_SETTING.to_string(),
                value: number(params.gap_s),
            },
        ],
        ..DeviceSettings::default()
    }
}

fn number(value: f64) -> serde_json::Value {
    serde_json::Number::from_f64(value).map_or(serde_json::Value::Null, serde_json::Value::Number)
}

fn ranged(name: &str, value: &serde_json::Value, min: f64, max: f64) -> Result<f64, DeviceError> {
    let number = value
        .as_f64()
        .ok_or_else(|| DeviceError::Unsupported(format!("`{name}` must be a number")))?;
    if !number.is_finite() || number < min || number > max {
        return Err(DeviceError::Unsupported(format!(
            "`{name}` {number} is outside {min}..={max}"
        )));
    }
    Ok(number)
}

impl SdrDevice for SignalGen {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn settings(&self) -> &DeviceSettings {
        &self.settings
    }

    fn apply(&mut self, settings: &DeviceSettings) -> Result<(), DeviceError> {
        check_stream_settings(settings, &self.capabilities)?;
        let mut next = self.params;
        let mut picked_signal = false;
        for extra in &settings.extra {
            match extra.name.as_str() {
                SIGNAL_SETTING => {
                    let id = extra.value.as_str().ok_or_else(|| {
                        DeviceError::Unsupported(format!("`{SIGNAL_SETTING}` must be a name"))
                    })?;
                    next.signal = next.catalog.find(id).ok_or_else(|| {
                        DeviceError::Unsupported(format!("no signal called `{id}`"))
                    })?;
                    picked_signal = true;
                }
                OFFSET_SETTING => {
                    next.offset_hz =
                        ranged(OFFSET_SETTING, &extra.value, -MAX_OFFSET_HZ, MAX_OFFSET_HZ)?;
                }
                LEVEL_SETTING => {
                    next.level_db = ranged(LEVEL_SETTING, &extra.value, MIN_LEVEL_DB, 0.0)?;
                }
                NOISE_SETTING => {
                    next.noise_db = ranged(NOISE_SETTING, &extra.value, NOISE_OFF_DB, 0.0)?;
                }
                GAP_SETTING => next.gap_s = ranged(GAP_SETTING, &extra.value, 0.0, MAX_GAP_S)?,
                other => return Err(DeviceError::Unsupported(format!("extra `{other}`"))),
            }
        }
        if let Some(rate) = settings.sample_rate {
            if !self.capabilities.sample_rates.contains(&rate) {
                return Err(DeviceError::Unsupported(format!("sample_rate {rate}")));
            }
            next.rate_hz = rate;
        } else if picked_signal {
            next.rate_hz = next.signal.rate_hz;
        }
        if let Some(hz) = settings.center_hz
            && !self
                .capabilities
                .freq_ranges
                .iter()
                .any(|range| range.min <= hz && hz <= range.max)
        {
            return Err(DeviceError::Unsupported(format!(
                "center_hz {hz} outside tuner range"
            )));
        }
        self.params = next;
        let center_hz = settings.center_hz.or(self.settings.center_hz);
        self.settings = DeviceSettings {
            center_hz,
            ..settings_of(&next)
        };
        self.shared.store(Arc::new(next));
        Ok(())
    }

    fn rx_start(&mut self, sinks: Vec<RxSink>) -> Result<(), DeviceError> {
        let mut sink = single_rx_sink(sinks)?;
        let shared = self.shared.clone();
        self.worker.start("sdrmm-generator-rx", move |running| {
            let mut rendered: Option<(Params, Vec<Complex<f32>>)> = None;
            let mut at = 0usize;
            let mut next_wake = Instant::now();
            while running.load(Ordering::Acquire) {
                let params = *shared.load_full();
                let stale = rendered
                    .as_ref()
                    .is_none_or(|(built, _)| !built.renders_the_same_as(&params));
                if stale {
                    match renderable(&params) {
                        Ok(block) => rendered = Some((params, block)),
                        Err(error) => {
                            sink.fail(error);
                            return;
                        }
                    }
                    at = 0;
                    next_wake = Instant::now();
                }
                let Some((_, block)) = rendered.as_ref() else {
                    return;
                };
                let step = ((params.rate_hz * BLOCK_SECS).round() as usize).max(1);
                let end = (at + step).min(block.len());
                sink.push(&block[at..end]);
                let sent = end - at;
                at = end;
                let mut idle = 0.0;
                if at >= block.len() {
                    at = 0;
                    idle = params.gap_s;
                }
                next_wake += Duration::from_secs_f64(sent as f64 / params.rate_hz + idle);
                let now = Instant::now();
                if next_wake > now {
                    std::thread::sleep(next_wake - now);
                } else {
                    next_wake = now;
                }
            }
        })
    }

    fn rx_stop(&mut self) {
        self.worker.stop();
    }
}

fn renderable(params: &Params) -> Result<Vec<Complex<f32>>, DeviceError> {
    let iq = render(params);
    if iq.is_empty() {
        tracing::warn!(
            signal = params.signal.id,
            "the generator produced no samples"
        );
        return Err(DeviceError::Io(format!(
            "the {} generator produced no samples",
            params.signal.id
        )));
    }
    Ok(iq)
}

fn render(params: &Params) -> Vec<Complex<f32>> {
    let mut iq = (params.signal.render)();
    if iq.is_empty() {
        return iq;
    }
    if params.rate_hz != params.signal.rate_hz {
        iq = synth::resample(&iq, params.signal.rate_hz, params.rate_hz);
    }
    if params.offset_hz != 0.0 {
        synth::shift(&mut iq, params.offset_hz, params.rate_hz);
    }
    synth::scale(&mut iq, amplitude(params.level_db));
    if params.noise_db > NOISE_OFF_DB {
        synth::add_noise(&mut iq, 0x5DEE_CE66, amplitude(params.noise_db));
    }
    iq
}

fn amplitude(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, time::Duration};

    use sdrmm_modem::synth::EMPTY_CATALOG;

    use sdrmm_device::DeviceDriver;
    use sdrmm_wire::{GENERATOR_PREFIX, generator_key};

    use super::*;
    use crate::VirtualDriver;

    fn tone() -> Vec<Complex<f32>> {
        vec![Complex::new(1.0, 0.0); 4_800]
    }

    fn chirp() -> Vec<Complex<f32>> {
        synth::ook(&vec![1.0f32; 9_600], 1_000.0, 48_000.0)
    }

    fn keyed() -> Vec<Complex<f32>> {
        synth::fsk(&[true, false, true, false], 1_200.0, 4_500.0, 240_000.0)
    }

    fn nothing() -> Vec<Complex<f32>> {
        Vec::new()
    }

    static SIGNALS: &[Signal] = &[
        Signal {
            id: "tone",
            label: "Carrier tone",
            rate_hz: 48_000.0,
            render: tone,
        },
        Signal {
            id: "chirp",
            label: "Chirp",
            rate_hz: 96_000.0,
            render: chirp,
        },
        Signal {
            id: "keyed",
            label: "Keyed carrier",
            rate_hz: 240_000.0,
            render: keyed,
        },
    ];

    static CATALOG: Catalog = Catalog {
        signals: SIGNALS,
        default_id: "chirp",
        center_hz: 145_000_000.0,
    };

    static SILENT_SIGNALS: &[Signal] = &[Signal {
        id: "silent",
        label: "Silent",
        rate_hz: 48_000.0,
        render: nothing,
    }];

    static SILENT_CATALOG: Catalog = Catalog {
        signals: SILENT_SIGNALS,
        default_id: "silent",
        center_hz: 145_000_000.0,
    };

    fn extra(name: &str, value: serde_json::Value) -> DeviceSettings {
        DeviceSettings {
            extra: vec![ExtraValue {
                name: name.to_string(),
                value,
            }],
            ..DeviceSettings::default()
        }
    }

    fn signal(id: &str) -> DeviceSettings {
        extra(SIGNAL_SETTING, serde_json::Value::String(id.to_string()))
    }

    fn power(iq: &[Complex<f32>]) -> f64 {
        if iq.is_empty() {
            return 0.0;
        }
        iq.iter().map(|s| f64::from(s.norm_sqr())).sum::<f64>() / iq.len() as f64
    }

    fn generator() -> SignalGen {
        SignalGen::new(&CATALOG).expect("the catalog holds signals")
    }

    fn default_params() -> Params {
        Params::new(&CATALOG, CATALOG.default_signal().expect("a default"))
    }

    #[test]
    fn a_generator_that_synthesises_nothing_fails_instead_of_streaming_silence() {
        let silent = SILENT_CATALOG.default_signal().expect("a default");
        let error = renderable(&Params::new(&SILENT_CATALOG, silent)).expect_err("no samples");
        assert!(error.to_string().contains("silent"), "{error}");
        assert!(renderable(&default_params()).is_ok());
    }

    #[test]
    fn a_device_without_a_catalog_is_refused_instead_of_opening_empty() {
        assert!(SignalGen::new(&EMPTY_CATALOG).is_err());
        assert!(EMPTY_CATALOG.is_empty());
    }

    #[test]
    fn the_default_signal_sets_the_rate_the_device_offers() {
        let device = generator();
        let default = CATALOG.default_signal().expect("a default");
        assert_eq!(device.settings().sample_rate, Some(default.rate_hz));
        assert!(
            capabilities(&CATALOG)
                .sample_rates
                .contains(&default.rate_hz)
        );
        assert_eq!(options(&CATALOG).len(), SIGNALS.len());
    }

    #[test]
    fn picking_a_signal_snaps_the_rate_to_the_one_it_is_generated_at() {
        let mut device = generator();
        device.apply(&signal("keyed")).expect("keyed is a signal");
        assert_eq!(
            device.settings().sample_rate,
            CATALOG.find("keyed").map(|signal| signal.rate_hz)
        );
    }

    #[test]
    fn a_rate_the_operator_asked_for_outlives_the_next_signal_change() {
        let mut device = generator();
        device
            .apply(&DeviceSettings {
                sample_rate: Some(240_000.0),
                ..signal("tone")
            })
            .expect("a catalog rate with a signal");
        assert_eq!(device.settings().sample_rate, Some(240_000.0));
    }

    #[test]
    fn tuning_is_kept_when_only_a_signal_changes() {
        let mut device = generator();
        device
            .apply(&DeviceSettings {
                center_hz: Some(433_920_000.0),
                ..DeviceSettings::default()
            })
            .expect("in range");
        device.apply(&signal("tone")).expect("tone is a signal");
        assert_eq!(device.settings().center_hz, Some(433_920_000.0));
    }

    #[test]
    fn refuses_settings_no_generator_can_answer() {
        let mut device = generator();
        for bad in [
            signal("no-such-signal"),
            extra(SIGNAL_SETTING, serde_json::json!(7)),
            extra(OFFSET_SETTING, serde_json::json!(1e12)),
            extra(LEVEL_SETTING, serde_json::json!(6.0)),
            extra(NOISE_SETTING, serde_json::json!("loud")),
            extra(GAP_SETTING, serde_json::json!(-1.0)),
            DeviceSettings {
                sample_rate: Some(3_333.0),
                ..DeviceSettings::default()
            },
            DeviceSettings {
                center_hz: Some(9_000_000_000.0),
                ..DeviceSettings::default()
            },
        ] {
            assert!(
                matches!(device.apply(&bad), Err(DeviceError::Unsupported(_))),
                "must refuse {bad:?}"
            );
        }
    }

    #[test]
    fn level_and_noise_move_the_power_the_way_they_are_named() {
        let quiet = Params {
            level_db: -40.0,
            noise_db: NOISE_OFF_DB,
            ..default_params()
        };
        let loud = Params {
            level_db: 0.0,
            ..quiet
        };
        let noisy = Params {
            noise_db: -10.0,
            ..quiet
        };
        assert!(power(&render(&loud)) > power(&render(&quiet)) * 100.0);
        assert!(power(&render(&noisy)) > power(&render(&quiet)) * 10.0);
    }

    #[test]
    fn a_running_generator_hands_samples_over_in_real_time() {
        let mut device = generator();
        device.apply(&signal("tone")).expect("tone is a signal");
        let (tx, rx) = mpsc::channel();
        device
            .rx_start(vec![RxSink::new(move |samples, _| {
                let _ = tx.send(samples.to_vec());
            })])
            .expect("starts");
        let block = rx.recv_timeout(Duration::from_secs(2)).expect("a block");
        device.rx_stop();
        assert!(!block.is_empty());
        assert!(power(&block) > 1e-9);
    }

    #[test]
    fn a_generator_is_adopted_once_and_then_probed() {
        let driver = VirtualDriver::bare().with_signals(&CATALOG);
        assert!(driver.probe().is_empty());
        let key = generator_key("signal_gen:a1b2");
        assert_eq!(
            driver.resolve(&key).map(|info| info.id()),
            Some(format!("virtual:{key}"))
        );
        assert!(driver.resolve(&key).is_some());
        assert_eq!(driver.probe().len(), 1);
        assert!(
            driver
                .open(&VirtualDriver::generator_info(&CATALOG, &key))
                .is_ok()
        );
    }

    #[test]
    fn a_key_that_is_not_a_node_name_is_refused() {
        let driver = VirtualDriver::bare().with_signals(&CATALOG);
        for key in [
            String::new(),
            GENERATOR_PREFIX.to_string(),
            format!("{GENERATOR_PREFIX}has space"),
            format!("{GENERATOR_PREFIX}has:colon"),
            format!("{GENERATOR_PREFIX}../escape"),
            format!("{GENERATOR_PREFIX}{}", "x".repeat(65)),
        ] {
            assert!(driver.resolve(&key).is_none(), "{key} must be refused");
            assert!(
                driver
                    .open(&VirtualDriver::generator_info(&CATALOG, &key))
                    .is_err()
            );
        }
    }

    #[test]
    fn a_generator_is_offered_even_when_the_synthetic_radios_are_not() {
        let driver = VirtualDriver::for_build().with_signals(&CATALOG);
        let key = generator_key("gen_node");
        assert!(driver.resolve(&key).is_some());
        assert!(
            driver
                .open(&VirtualDriver::generator_info(&CATALOG, &key))
                .is_ok()
        );
    }

    #[test]
    fn the_adoption_list_has_a_ceiling() {
        let driver = VirtualDriver::bare().with_signals(&CATALOG);
        for index in 0..MAX_GENERATORS {
            assert!(
                driver
                    .resolve(&generator_key(&format!("n{index}")))
                    .is_some()
            );
        }
        assert!(driver.resolve(&generator_key("one-too-many")).is_none());
    }
}

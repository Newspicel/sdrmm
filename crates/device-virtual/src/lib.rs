pub mod bench;
pub mod generator;

use std::{
    sync::{Arc, Mutex, atomic::Ordering},
    time::{Duration, Instant},
};

use arc_swap::ArcSwap;
use num_complex::Complex;
use sdrmm_device::{
    DeviceDriver, DeviceError, RxSink, SdrDevice, SweepPlan, SweepSink, Worker,
    check_stream_settings, lock, single_rx_sink,
};
use sdrmm_dsp::scene::{LaneRenderer, LaneSetup};
use sdrmm_modem::synth::{Catalog, EMPTY_CATALOG};
use sdrmm_wire::{
    Capabilities, Coherence, DcArtifact, DeviceBackend, DeviceInfo, DeviceSettings, Duplex,
    GENERATOR_PREFIX, GainKind, GainStage, Range, StreamScope,
};

use bench::BenchDevice;
pub use bench::{
    BEARING_SETTING, BLOCK_LEN, BenchDeviceSpec, BenchWorld, Clutter, Echo, Emitter,
    LaneImpairments, LaneTruth, NOISE_SWITCH_LEAD_S, Path, Pilot, RADIUS_SETTING, ReportedGap,
    Ripple, Scene, Slip, Waveform, default_devices, default_scene, default_world,
};
use generator::SignalGen;
pub use generator::{
    GAP_SETTING, LEVEL_SETTING, MAX_GENERATORS, NOISE_OFF_DB, NOISE_SETTING, OFFSET_SETTING,
    SIGNAL_SETTING,
};

const DRIVER_ID: DeviceBackend = DeviceBackend::Virtual;
const MAX_GENERATOR_KEY_LEN: usize = 64;
const BAND_KEY: &str = "band";
const BLOCK_SECS: f64 = 0.025;

pub const NFM_CARRIER_OFFSET_HZ: f64 = 300_000.0;
pub const NFM_DEVIATION_HZ: f64 = 2_500.0;
pub const AM_CARRIER_OFFSET_HZ: f64 = -300_000.0;
pub const AM_MOD_DEPTH: f64 = 0.6;
pub const WFM_CARRIER_OFFSET_HZ: f64 = 600_000.0;
pub const WFM_DEVIATION_HZ: f64 = 75_000.0;
pub const MOD_TONE_HZ: f64 = 1_000.0;

pub const STREAM_MARKER_SPACING_HZ: f64 = 50_000.0;

pub const SWEEP_MARKER_HZ: f64 = 101_000_000.0;
const SWEEP_BLOCK_SAMPLES: usize = 4_096;

#[must_use]
pub fn stream_marker_offset_hz(stream: u32) -> f64 {
    STREAM_MARKER_SPACING_HZ * (f64::from(stream) + 1.0)
}

const MOD_CARRIER_AMP: f64 = 0.10;

const DEFAULT_CENTER_HZ: f64 = 100_000_000.0;
const DEFAULT_SAMPLE_RATE_HZ: f64 = 2_048_000.0;

const NOISE_SEED: u64 = 0x5DEE_CE66_D00D_1234;

pub struct VirtualDriver {
    synthetic_devices: bool,
    world: Arc<BenchWorld>,
    signals: &'static Catalog,
    adopted: Mutex<Vec<String>>,
}

impl Default for VirtualDriver {
    fn default() -> Self {
        Self::for_build()
    }
}

#[must_use]
pub const fn synthetic_devices_offered() -> bool {
    cfg!(debug_assertions)
}

impl VirtualDriver {
    #[must_use]
    pub fn new() -> Self {
        Self::with_world(default_world())
    }

    #[must_use]
    pub fn for_build() -> Self {
        Self {
            synthetic_devices: synthetic_devices_offered(),
            ..Self::with_world(default_world())
        }
    }

    #[must_use]
    pub fn bare() -> Self {
        Self {
            synthetic_devices: false,
            ..Self::with_world(default_world())
        }
    }

    #[must_use]
    pub fn with_world(world: Arc<BenchWorld>) -> Self {
        Self {
            synthetic_devices: true,
            world,
            signals: &EMPTY_CATALOG,
            adopted: Mutex::new(Vec::new()),
        }
    }

    #[must_use]
    pub fn with_signals(mut self, signals: &'static Catalog) -> Self {
        self.signals = signals;
        self
    }

    #[must_use]
    pub fn world(&self) -> &Arc<BenchWorld> {
        &self.world
    }

    #[must_use]
    pub fn generator_info(signals: &Catalog, key: &str) -> DeviceInfo {
        DeviceInfo {
            driver: DRIVER_ID,
            key: key.to_string(),
            label: "Signal generator".to_string(),
            serial: None,
            profile: Some(generator::capabilities(signals).profile()),
        }
    }

    fn generator_key_valid(&self, key: &str) -> bool {
        let Some(node) = key.strip_prefix(GENERATOR_PREFIX) else {
            return false;
        };
        !self.signals.is_empty()
            && !node.is_empty()
            && key.len() <= MAX_GENERATOR_KEY_LEN
            && node
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    }

    fn band_info() -> DeviceInfo {
        DeviceInfo {
            driver: DRIVER_ID,
            key: BAND_KEY.to_string(),
            label: "Test band (virtual)".to_string(),
            serial: None,
            profile: Some(band_capabilities().profile()),
        }
    }

    fn marker_info(shape: &MarkerShape) -> DeviceInfo {
        DeviceInfo {
            driver: DRIVER_ID,
            key: shape.key.to_string(),
            label: shape.label.to_string(),
            serial: None,
            profile: Some(marker_capabilities(shape).profile()),
        }
    }
}

impl DeviceDriver for VirtualDriver {
    fn id(&self) -> DeviceBackend {
        DRIVER_ID
    }

    fn probe(&self) -> Vec<DeviceInfo> {
        let mut infos = Vec::new();
        if self.synthetic_devices {
            infos.push(Self::band_info());
            infos.extend(MARKER_SHAPES.iter().map(Self::marker_info));
            infos.extend(self.world.infos());
        }
        infos.extend(
            lock(&self.adopted)
                .iter()
                .map(|key| Self::generator_info(self.signals, key)),
        );
        infos
    }

    fn open(&self, info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        if info.key.starts_with(GENERATOR_PREFIX) {
            if !self.generator_key_valid(&info.key) {
                return Err(DeviceError::NotFound(format!("{DRIVER_ID}:{}", info.key)));
            }
            return Ok(Box::new(SignalGen::new(self.signals)?));
        }
        if !self.synthetic_devices {
            return Err(DeviceError::NotFound(format!("{DRIVER_ID}:{}", info.key)));
        }
        if info.key == BAND_KEY {
            return Ok(Box::new(BandGen::new()));
        }
        match MARKER_SHAPES.iter().find(|shape| shape.key == info.key) {
            Some(shape) => Ok(Box::new(MarkerGen::new(shape))),
            None => Ok(Box::new(BenchDevice::open(self.world.clone(), &info.key)?)),
        }
    }

    fn resolve(&self, key: &str) -> Option<DeviceInfo> {
        if !self.generator_key_valid(key) {
            return None;
        }
        let mut adopted = lock(&self.adopted);
        if !adopted.iter().any(|held| held == key) {
            if adopted.len() >= MAX_GENERATORS {
                return None;
            }
            adopted.push(key.to_string());
        }
        Some(Self::generator_info(self.signals, key))
    }
}

#[must_use]
pub fn render(sample_rate: f64, n: usize) -> Vec<Complex<f32>> {
    let mut block = vec![Complex::new(0.0, 0.0); n];
    Generator::new().fill(&mut block, sample_rate);
    block
}

#[derive(Clone, Copy)]
struct BandParams {
    sample_rate: f64,
}

pub struct BandGen {
    capabilities: Capabilities,
    settings: DeviceSettings,
    shared: Arc<ArcSwap<BandParams>>,
    worker: Worker,
    sweeper: Worker,
}

impl Default for BandGen {
    fn default() -> Self {
        Self::new()
    }
}

#[must_use]
fn band_capabilities() -> Capabilities {
    Capabilities {
        freq_ranges: vec![Range {
            min: 0.0,
            max: 6_000_000_000.0,
            step: None,
        }],
        sample_rates: vec![
            250_000.0,
            1_024_000.0,
            2_000_000.0,
            2_048_000.0,
            2_400_000.0,
            3_200_000.0,
            4_000_000.0,
            8_000_000.0,
            10_000_000.0,
            20_000_000.0,
        ],
        sample_rate_ranges: Vec::new(),
        gains: vec![GainStage::new(
            GainKind::Tuner,
            Range {
                min: 0.0,
                max: 49.6,
                step: Some(0.1),
            },
        )],
        antennas: vec!["RX".to_string()],
        bandwidths: Vec::new(),
        bandwidth_ranges: Vec::new(),
        bandwidth_auto: false,
        bias_tee: false,
        agc: sdrmm_wire::Agc::None,
        extra: Vec::new(),
        ppm: false,
        duplex: Duplex::RxOnly,
        rx_streams: 1,
        tx_streams: 0,
        per_stream: StreamScope::default(),
        directional: None,
        dc_artifact: DcArtifact::Operator,
        hardware_sweep: true,
        coherence: sdrmm_wire::Coherence::None,
        noise_source: sdrmm_wire::NoiseSource::None,
        rx_inputs: Vec::new(),
        clean_fraction: None,
        retune_keeps_phase: false,
        bank: None,
    }
}

fn sweep_centers(plan: &SweepPlan) -> Vec<f64> {
    let step = plan.sample_rate_hz;
    let mut centers = Vec::new();
    for band in &plan.bands {
        let mut center = band.start_hz + step / 2.0;
        while center - step / 2.0 < band.stop_hz {
            centers.push(center);
            center += step;
        }
    }
    centers
}

impl BandGen {
    #[must_use]
    pub fn new() -> Self {
        Self {
            capabilities: band_capabilities(),
            settings: default_settings(),
            shared: Arc::new(ArcSwap::from_pointee(BandParams {
                sample_rate: DEFAULT_SAMPLE_RATE_HZ,
            })),
            worker: Worker::new(),
            sweeper: Worker::new(),
        }
    }

    fn sample_rate(&self) -> f64 {
        self.settings.sample_rate.unwrap_or(DEFAULT_SAMPLE_RATE_HZ)
    }
}

fn default_settings() -> DeviceSettings {
    DeviceSettings {
        center_hz: Some(DEFAULT_CENTER_HZ),
        sample_rate: Some(DEFAULT_SAMPLE_RATE_HZ),
        ..DeviceSettings::default()
    }
}

fn validate_tune(
    capabilities: &Capabilities,
    settings: &DeviceSettings,
) -> Result<(), DeviceError> {
    check_stream_settings(settings, capabilities)?;
    if let Some(rate) = settings.sample_rate
        && (rate <= 0.0 || !capabilities.sample_rates.contains(&rate))
    {
        return Err(DeviceError::Unsupported(format!("sample_rate {rate}")));
    }
    let in_range = |f: f64| {
        capabilities
            .freq_ranges
            .iter()
            .any(|r| r.min <= f && f <= r.max)
    };
    if let Some(f) = settings.center_hz
        && !in_range(f)
    {
        return Err(DeviceError::Unsupported(format!(
            "center_hz {f} outside tuner range"
        )));
    }
    for entry in &settings.streams {
        if let Some(f) = entry.center_hz
            && !in_range(f)
        {
            return Err(DeviceError::Unsupported(format!(
                "streams[{}].center_hz {f} outside tuner range",
                entry.stream
            )));
        }
    }
    Ok(())
}

impl SdrDevice for BandGen {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn settings(&self) -> &DeviceSettings {
        &self.settings
    }

    fn apply(&mut self, settings: &DeviceSettings) -> Result<(), DeviceError> {
        validate_tune(&self.capabilities, settings)?;
        self.settings.merge_from(settings);
        self.shared.store(Arc::new(BandParams {
            sample_rate: self.sample_rate(),
        }));
        Ok(())
    }

    fn rx_start(&mut self, sinks: Vec<RxSink>) -> Result<(), DeviceError> {
        let mut sink = single_rx_sink(sinks)?;
        let shared = self.shared.clone();
        self.worker.start("sdrmm-band-rx", move |running| {
            let mut generator = Generator::new();
            let mut block: Vec<Complex<f32>> = Vec::new();
            let mut next = Instant::now();
            while running.load(Ordering::Acquire) {
                let params = *shared.load_full();
                let n = ((params.sample_rate * BLOCK_SECS).round() as usize).max(1);
                block.resize(n, Complex::new(0.0, 0.0));
                generator.fill(&mut block, params.sample_rate);
                sink.push(&block);

                next += Duration::from_secs_f64(n as f64 / params.sample_rate);
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                } else {
                    next = now;
                }
            }
        })
    }

    fn rx_stop(&mut self) {
        self.worker.stop();
    }

    fn sweep_start(&mut self, plan: &SweepPlan, mut sink: SweepSink) -> Result<(), DeviceError> {
        plan.check()?;
        let centers = sweep_centers(plan);
        if centers.is_empty() {
            return Err(DeviceError::Unsupported(
                "the sweep bands hold no tuning at this sample rate".to_string(),
            ));
        }
        let sample_rate = plan.sample_rate_hz;
        self.sweeper.start("sdrmm-band-sweep", move |running| {
            let mut generator = Generator::stream_marker(0);
            let mut block = vec![Complex::new(0.0f32, 0.0); SWEEP_BLOCK_SAMPLES];
            let mut next = Instant::now();
            let mut at = 0usize;
            while running.load(Ordering::Acquire) {
                let center = centers[at];
                at = (at + 1) % centers.len();
                generator.set_marker_offset_hz(SWEEP_MARKER_HZ - center);
                generator.fill(&mut block, sample_rate);
                sink.push(center, &block);

                next += Duration::from_secs_f64(block.len() as f64 / sample_rate);
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                } else {
                    next = now;
                }
            }
        })
    }

    fn sweep_stop(&mut self) {
        self.sweeper.stop();
    }
}

pub struct MarkerShape {
    pub key: &'static str,
    pub label: &'static str,
    pub duplex: Duplex,
    pub rx_streams: u32,
    pub tx_streams: u32,
    pub per_stream: StreamScope,
    pub coherence: Coherence,
}

pub const MARKER_SHAPES: [MarkerShape; 3] = [
    MarkerShape {
        key: "quad",
        label: "Receiver ×4 (virtual)",
        duplex: Duplex::RxOnly,
        rx_streams: 4,
        tx_streams: 0,
        per_stream: StreamScope {
            tuning: false,
            gain: true,
            antenna: false,
            agc: false,
        },
        coherence: Coherence::None,
    },
    MarkerShape {
        key: "transceiver",
        label: "Transceiver 2×2 (virtual)",
        duplex: Duplex::Full,
        rx_streams: 2,
        tx_streams: 2,
        per_stream: StreamScope {
            tuning: true,
            gain: true,
            antenna: true,
            agc: false,
        },
        coherence: Coherence::None,
    },
    MarkerShape {
        key: "halfduplex",
        label: "Half-duplex 1×1 (virtual)",
        duplex: Duplex::Half,
        rx_streams: 1,
        tx_streams: 1,
        per_stream: StreamScope {
            tuning: false,
            gain: false,
            antenna: false,
            agc: false,
        },
        coherence: Coherence::None,
    },
];

fn marker_capabilities(shape: &MarkerShape) -> Capabilities {
    Capabilities {
        duplex: shape.duplex,
        rx_streams: shape.rx_streams,
        tx_streams: shape.tx_streams,
        per_stream: shape.per_stream,
        coherence: shape.coherence,
        noise_source: sdrmm_wire::NoiseSource::None,
        rx_inputs: Vec::new(),
        extra: Vec::new(),
        hardware_sweep: false,
        ..band_capabilities()
    }
}

fn marker_offsets(settings: &DeviceSettings, capabilities: &Capabilities) -> Vec<f64> {
    let radio_center = settings.center_hz.unwrap_or(DEFAULT_CENTER_HZ);
    (0..capabilities.rx_streams)
        .map(|stream| {
            let lane_center = settings
                .for_stream(stream, &capabilities.per_stream)
                .center_hz
                .unwrap_or(radio_center);
            stream_marker_offset_hz(stream) + radio_center - lane_center
        })
        .collect()
}

pub struct MarkerGen {
    capabilities: Capabilities,
    settings: DeviceSettings,
    shared: Arc<ArcSwap<MarkerParams>>,
    worker: Worker,
}

struct MarkerParams {
    sample_rate: f64,
    marker_offsets: Vec<f64>,
}

impl MarkerGen {
    fn new(shape: &MarkerShape) -> Self {
        let capabilities = marker_capabilities(shape);
        let settings = default_settings();
        let shared = Arc::new(ArcSwap::from_pointee(marker_params(
            &settings,
            &capabilities,
        )));
        Self {
            capabilities,
            settings,
            shared,
            worker: Worker::new(),
        }
    }
}

fn marker_params(settings: &DeviceSettings, capabilities: &Capabilities) -> MarkerParams {
    MarkerParams {
        sample_rate: settings.sample_rate.unwrap_or(DEFAULT_SAMPLE_RATE_HZ),
        marker_offsets: marker_offsets(settings, capabilities),
    }
}

impl SdrDevice for MarkerGen {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn settings(&self) -> &DeviceSettings {
        &self.settings
    }

    fn apply(&mut self, settings: &DeviceSettings) -> Result<(), DeviceError> {
        validate_tune(&self.capabilities, settings)?;
        if let Some(extra) = settings.extra.first() {
            return Err(DeviceError::Unsupported(format!("extra `{}`", extra.name)));
        }
        self.settings.merge_from(settings);
        self.shared
            .store(Arc::new(marker_params(&self.settings, &self.capabilities)));
        Ok(())
    }

    fn rx_start(&mut self, sinks: Vec<RxSink>) -> Result<(), DeviceError> {
        let expected = self.capabilities.rx_streams as usize;
        if sinks.len() != expected {
            return Err(DeviceError::Unsupported(format!(
                "this device has {expected} rx streams, got {} sinks",
                sinks.len()
            )));
        }
        let shared = self.shared.clone();
        self.worker.start("sdrmm-marker-rx", move |running| {
            let mut lanes: Vec<(Generator, RxSink)> = sinks
                .into_iter()
                .enumerate()
                .map(|(stream, sink)| (Generator::stream_marker(stream as u32), sink))
                .collect();
            let mut block: Vec<Complex<f32>> = Vec::new();
            let mut next = Instant::now();
            while running.load(Ordering::Acquire) {
                let params = shared.load_full();
                let n = ((params.sample_rate * BLOCK_SECS).round() as usize).max(1);
                block.resize(n, Complex::new(0.0, 0.0));
                for (stream, (generator, sink)) in lanes.iter_mut().enumerate() {
                    let offset = params
                        .marker_offsets
                        .get(stream)
                        .copied()
                        .unwrap_or_else(|| stream_marker_offset_hz(stream as u32));
                    generator.set_marker_offset_hz(offset);
                    generator.fill(&mut block, params.sample_rate);
                    sink.push(&block);
                }

                next += Duration::from_secs_f64(n as f64 / params.sample_rate);
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                } else {
                    next = now;
                }
            }
        })
    }

    fn rx_stop(&mut self) {
        self.worker.stop();
    }
}

const LFO_HZ: f64 = 0.08;
const DRIFT_SPAN: f64 = 0.35;
const DRIFT_AMP_DB: f64 = -20.0;
const NOISE_DBFS: f64 = -46.2;
const TONES: [(f64, f64); 3] = [(0.15, 0.15), (-0.30, 0.08), (0.05, 0.05)];

fn emitter(offset_hz: f64, power_db: f64, waveform: Waveform) -> Emitter {
    Emitter {
        azimuth_deg: 0.0,
        elevation_deg: 0.0,
        offset_hz,
        power_dbfs: power_db,
        waveform,
        paths: Vec::new(),
    }
}

fn band_emitters(sample_rate: f64) -> Vec<Emitter> {
    let mut emitters: Vec<Emitter> = TONES
        .iter()
        .map(|&(fraction, amp)| emitter(fraction * sample_rate, decibels(amp), Waveform::Tone))
        .collect();
    emitters.push(emitter(
        0.0,
        DRIFT_AMP_DB,
        Waveform::Fm {
            deviation_hz: DRIFT_SPAN * sample_rate,
            rate_hz: LFO_HZ,
        },
    ));
    emitters.extend(
        MOD_CARRIERS
            .iter()
            .map(|&(offset_hz, waveform)| emitter(offset_hz, decibels(MOD_CARRIER_AMP), waveform)),
    );
    emitters
}

const MOD_CARRIERS: [(f64, Waveform); 3] = [
    (
        NFM_CARRIER_OFFSET_HZ,
        Waveform::Fm {
            deviation_hz: NFM_DEVIATION_HZ,
            rate_hz: MOD_TONE_HZ,
        },
    ),
    (
        AM_CARRIER_OFFSET_HZ,
        Waveform::Am {
            depth: AM_MOD_DEPTH,
            rate_hz: MOD_TONE_HZ,
        },
    ),
    (
        WFM_CARRIER_OFFSET_HZ,
        Waveform::Fm {
            deviation_hz: WFM_DEVIATION_HZ,
            rate_hz: MOD_TONE_HZ,
        },
    ),
];

fn decibels(amplitude: f64) -> f64 {
    20.0 * amplitude.log10()
}

fn scene_of(emitters: Vec<Emitter>, sample_rate: f64, seed: u64) -> Scene {
    Scene {
        positions: vec![[0.0, 0.0, 0.0]],
        emitters,
        echoes: Vec::new(),
        clutter: Clutter::default(),
        thermal_dbfs: NOISE_DBFS,
        noise_source_dbfs: NOISE_DBFS,
        noise_bandwidth: sample_rate,
        seed,
    }
}

struct Generator {
    renderer: LaneRenderer,
    marker: Option<f64>,
    seed: u64,
    built_at: f64,
    sample: i64,
}

impl Generator {
    fn new() -> Self {
        Self {
            renderer: LaneRenderer::new(NOISE_SEED),
            marker: None,
            seed: NOISE_SEED,
            built_at: 0.0,
            sample: 0,
        }
    }

    fn stream_marker(stream: u32) -> Self {
        let seed = NOISE_SEED ^ (u64::from(stream) << 32);
        Self {
            renderer: LaneRenderer::new(seed),
            marker: Some(stream_marker_offset_hz(stream)),
            seed,
            built_at: 0.0,
            sample: 0,
        }
    }

    fn set_marker_offset_hz(&mut self, offset_hz: f64) {
        if self.marker != Some(offset_hz) {
            self.marker = Some(offset_hz);
            self.built_at = 0.0;
        }
    }

    fn emitters(&self, sample_rate: f64) -> Vec<Emitter> {
        match self.marker {
            Some(offset_hz) => vec![emitter(
                offset_hz,
                decibels(MOD_CARRIER_AMP),
                Waveform::Fm {
                    deviation_hz: NFM_DEVIATION_HZ,
                    rate_hz: MOD_TONE_HZ,
                },
            )],
            None => band_emitters(sample_rate),
        }
    }

    fn fill(&mut self, block: &mut [Complex<f32>], sample_rate: f64) {
        if self.built_at != sample_rate {
            let scene = scene_of(self.emitters(sample_rate), sample_rate, self.seed);
            let impairments = LaneImpairments::default();
            self.renderer.configure(
                &scene,
                &LaneSetup {
                    position: scene.positions[0],
                    center_hz: DEFAULT_CENTER_HZ,
                    radio_center_hz: DEFAULT_CENTER_HZ,
                    sample_rate,
                    impairments: &impairments,
                    gain_setting_db: 0.0,
                    scramble_rad: 0.0,
                    pilot: None,
                    noise_seed: None,
                    isolated: false,
                },
            );
            self.built_at = sample_rate;
        }
        let step_s = 1.0 / sample_rate;
        self.renderer.render(
            self.sample,
            self.sample as f64 * step_s,
            step_s,
            false,
            block,
        );
        self.sample += block.len() as i64;
    }
}

#[cfg(test)]
mod tests {
    use std::{
        f64::consts::TAU,
        sync::{OnceLock, mpsc},
        time::Duration,
    };

    use sdrmm_wire::{BandwidthSetting, GainKind, GainValue, StreamSettings};

    use super::*;

    const SPECTRUM_FS: f64 = 2_400_000.0;
    const SPECTRUM_LEN: usize = 1 << 20;

    fn power_spectrum(n: usize, sample_rate: f64) -> Vec<f64> {
        let mut generator = Generator::new();
        let mut block = vec![Complex::new(0.0f32, 0.0f32); n];
        generator.fill(&mut block, sample_rate);
        spectrum_of(&block)
    }

    fn spectrum_of(samples: &[Complex<f32>]) -> Vec<f64> {
        let n = samples.len();
        let mut buf: Vec<Complex<f64>> = samples
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let w = 0.5 - 0.5 * (TAU * i as f64 / n as f64).cos();
                Complex::new(f64::from(s.re) * w, f64::from(s.im) * w)
            })
            .collect();
        sdrmm_dsp::fft::Transform::forward(n).process(&mut buf);
        buf.iter().map(|c| c.norm_sqr()).collect()
    }

    fn band_power(power: &[f64], sample_rate: f64, center_hz: f64, half_width_hz: f64) -> f64 {
        let n = power.len() as f64;
        let bin_hz = sample_rate / n;
        power
            .iter()
            .enumerate()
            .filter(|(k, _)| {
                let k = *k as f64;
                let freq = if k <= n / 2.0 { k } else { k - n } * bin_hz;
                (freq - center_hz).abs() <= half_width_hz
            })
            .map(|(_, p)| p)
            .sum()
    }

    fn shared_spectrum() -> &'static [f64] {
        static SPECTRUM: OnceLock<Vec<f64>> = OnceLock::new();
        SPECTRUM.get_or_init(|| power_spectrum(SPECTRUM_LEN, SPECTRUM_FS))
    }

    #[test]
    fn modulated_carriers_land_at_their_offsets() {
        let power = shared_spectrum();
        for (offset, half_width) in [
            (NFM_CARRIER_OFFSET_HZ, 10_000.0),
            (AM_CARRIER_OFFSET_HZ, 10_000.0),
            (WFM_CARRIER_OFFSET_HZ, 100_000.0),
        ] {
            let in_band = band_power(power, SPECTRUM_FS, offset, half_width);
            let reference = band_power(
                power,
                SPECTRUM_FS,
                offset + offset.signum() * 4.0 * half_width,
                half_width,
            );
            assert!(
                in_band > 100.0 * reference,
                "no carrier near {offset} Hz: in-band {in_band:.3e}, reference {reference:.3e}"
            );
        }
    }

    #[test]
    fn modulation_bandwidths_are_roughly_right() {
        let power = shared_spectrum();

        let wfm_wide = band_power(power, SPECTRUM_FS, WFM_CARRIER_OFFSET_HZ, 100_000.0);
        let wfm_narrow = band_power(power, SPECTRUM_FS, WFM_CARRIER_OFFSET_HZ, 10_000.0);
        assert!(
            wfm_narrow < 0.5 * wfm_wide,
            "WFM energy not wideband: {wfm_narrow:.3e} of {wfm_wide:.3e} within ±10 kHz"
        );

        for (offset, label) in [(NFM_CARRIER_OFFSET_HZ, "NFM"), (AM_CARRIER_OFFSET_HZ, "AM")] {
            let wide = band_power(power, SPECTRUM_FS, offset, 30_000.0);
            let narrow = band_power(power, SPECTRUM_FS, offset, 10_000.0);
            assert!(
                narrow > 0.95 * wide,
                "{label} energy not confined to ±10 kHz: {narrow:.3e} of {wide:.3e}"
            );
        }
    }

    #[test]
    fn output_is_identical_regardless_of_block_size() {
        let fs = 2_048_000.0;
        let mut whole = vec![Complex::new(0.0f32, 0.0f32); 8192];
        Generator::new().fill(&mut whole, fs);

        let mut generator = Generator::new();
        let mut chunked = vec![Complex::new(0.0f32, 0.0f32); 8192];
        for chunk in chunked.chunks_mut(999) {
            generator.fill(chunk, fs);
        }

        for (i, (a, b)) in whole.iter().zip(&chunked).enumerate() {
            assert!(
                (a.re - b.re).abs() < 1e-4 && (a.im - b.im).abs() < 1e-4,
                "sample {i} diverged: {a} vs {b}"
            );
        }
    }

    #[test]
    fn carrier_past_nyquist_is_muted_not_aliased() {
        let fs = 1_024_000.0;
        let power = power_spectrum(1 << 19, fs);
        let aliased = band_power(&power, fs, -424_000.0, 100_000.0);
        let nfm = band_power(&power, fs, NFM_CARRIER_OFFSET_HZ, 10_000.0);
        assert!(
            nfm > 100.0 * aliased,
            "WFM aliased past Nyquist: {aliased:.3e} vs NFM {nfm:.3e}"
        );
    }

    #[test]
    fn probe_lists_band_marker_and_bench_radios() {
        let d = VirtualDriver::new();
        let ids: Vec<String> = d.probe().iter().map(DeviceInfo::id).collect();
        assert_eq!(
            ids,
            [
                "virtual:band",
                "virtual:quad",
                "virtual:transceiver",
                "virtual:halfduplex",
                "virtual:kraken5",
                "virtual:array4",
                "virtual:dongle1",
                "virtual:dongle2",
            ]
        );
    }

    fn assert_synthetic_policy(d: &VirtualDriver, enabled: bool) {
        let infos = d.probe();
        let expected = std::iter::once(VirtualDriver::band_info())
            .chain(MARKER_SHAPES.iter().map(VirtualDriver::marker_info))
            .chain(d.world().infos());
        for info in expected {
            assert_eq!(
                infos.iter().any(|probed| probed.key == info.key),
                enabled,
                "{} has the wrong probe visibility",
                info.id()
            );
            if enabled {
                d.open(&info).unwrap();
            } else {
                assert!(matches!(
                    d.open(&info),
                    Err(DeviceError::NotFound(id)) if id == info.id()
                ));
            }
        }
    }

    #[test]
    fn application_build_policy_matches_the_profile() {
        assert_synthetic_policy(&VirtualDriver::for_build(), cfg!(debug_assertions));
        assert_synthetic_policy(&VirtualDriver::default(), cfg!(debug_assertions));
        assert_synthetic_policy(&VirtualDriver::new(), true);
    }

    #[test]
    fn a_production_build_shows_no_synthetic_radios() {
        let production = VirtualDriver::for_build();
        assert_eq!(
            production.probe().is_empty(),
            !cfg!(debug_assertions),
            "synthetic radios belong to debug builds only"
        );
    }

    fn open_virtual(key: &str) -> Box<dyn SdrDevice> {
        let driver = VirtualDriver::new();
        let infos = driver.probe();
        let info = infos.iter().find(|info| info.key == key).expect("probed");
        driver.open(info).expect("open")
    }

    #[test]
    fn marker_radios_report_their_shapes_at_probe_and_open() {
        let driver = VirtualDriver::new();
        let infos = driver.probe();
        for shape in &MARKER_SHAPES {
            let info = infos.iter().find(|info| info.key == shape.key).unwrap();
            assert_eq!(info.label, shape.label);
            assert_eq!(info.id(), format!("virtual:{}", shape.key));
            let profile = info.profile.as_ref().unwrap();
            assert_eq!(profile.duplex, shape.duplex, "{}", shape.key);
            assert_eq!(profile.rx_streams, shape.rx_streams, "{}", shape.key);
            assert_eq!(profile.tx_streams, shape.tx_streams, "{}", shape.key);
            assert_eq!(profile.per_stream, shape.per_stream, "{}", shape.key);

            let dev = driver.open(info).unwrap();
            let caps = dev.capabilities();
            assert_eq!(caps.duplex, shape.duplex, "{}", shape.key);
            assert_eq!(caps.rx_streams, shape.rx_streams, "{}", shape.key);
            assert_eq!(caps.tx_streams, shape.tx_streams, "{}", shape.key);
            assert_eq!(caps.per_stream, shape.per_stream, "{}", shape.key);
            assert_eq!(caps.sample_rates, band_capabilities().sample_rates);
            assert_eq!(caps.freq_ranges, band_capabilities().freq_ranges);
        }
    }

    #[test]
    fn marker_rx_start_requires_one_sink_per_stream() {
        let mut dev = open_virtual("quad");
        for wrong in [0usize, 1, 3, 5] {
            let sinks = (0..wrong).map(|_| RxSink::new(|_, _| {})).collect();
            match dev.rx_start(sinks) {
                Err(DeviceError::Unsupported(message)) => {
                    assert!(message.contains('4'), "{message}");
                }
                other => panic!("{wrong} sinks must be Unsupported, got {other:?}"),
            }
        }
        dev.rx_start((0..4).map(|_| RxSink::new(|_, _| {})).collect())
            .unwrap();
        dev.rx_stop();
        dev.rx_start((0..4).map(|_| RxSink::new(|_, _| {})).collect())
            .unwrap();
        dev.rx_stop();
    }

    #[test]
    fn band_refuses_more_than_one_sink() {
        let mut dev = BandGen::new();
        let sinks = vec![RxSink::new(|_, _| {}), RxSink::new(|_, _| {})];
        assert!(matches!(
            dev.rx_start(sinks),
            Err(DeviceError::Unsupported(_))
        ));
        dev.rx_start(vec![RxSink::new(|_, _| {})]).unwrap();
        dev.rx_stop();
    }

    #[test]
    fn each_stream_carries_its_own_marker() {
        const STREAMS: usize = 4;
        const N: usize = 1 << 17;

        let mut dev = open_virtual("quad");
        let mut receivers = Vec::new();
        let sinks = (0..STREAMS)
            .map(|_| {
                let (tx, rx) = mpsc::channel::<Vec<Complex<f32>>>();
                receivers.push(rx);
                RxSink::new(move |s, _| {
                    let _ = tx.send(s.to_vec());
                })
            })
            .collect();
        dev.rx_start(sinks).unwrap();
        let rate = dev.settings().sample_rate.unwrap();
        let streams: Vec<Vec<Complex<f32>>> = receivers
            .iter()
            .map(|rx| {
                let mut samples = Vec::new();
                while samples.len() < N {
                    samples.extend(rx.recv_timeout(Duration::from_secs(2)).unwrap());
                }
                samples.truncate(N);
                samples
            })
            .collect();
        dev.rx_stop();

        for (stream, samples) in streams.iter().enumerate() {
            let power = spectrum_of(samples);
            let own = band_power(
                &power,
                rate,
                stream_marker_offset_hz(stream as u32),
                10_000.0,
            );
            for other in 0..STREAMS {
                if other == stream {
                    continue;
                }
                let leaked = band_power(
                    &power,
                    rate,
                    stream_marker_offset_hz(other as u32),
                    10_000.0,
                );
                assert!(
                    own > 100.0 * leaked,
                    "stream {stream}: marker {other} not distinguishable \
                     (own {own:.3e}, other {leaked:.3e})"
                );
            }
        }
    }

    #[test]
    fn band_refuses_any_streams_entry() {
        let mut dev = BandGen::new();
        let delta = DeviceSettings {
            streams: vec![StreamSettings {
                stream: 0,
                ..StreamSettings::default()
            }],
            ..DeviceSettings::default()
        };
        match dev.apply(&delta) {
            Err(DeviceError::Unsupported(message)) => {
                assert!(message.contains("streams[0]"), "{message}");
            }
            other => panic!("a streams entry on the band must be Unsupported, got {other:?}"),
        }
        assert!(dev.settings().streams.is_empty());
    }

    #[test]
    fn quad_refuses_a_per_stream_center_but_takes_per_stream_gain() {
        let mut dev = open_virtual("quad");
        let retune = DeviceSettings {
            streams: vec![StreamSettings {
                stream: 1,
                center_hz: Some(101_000_000.0),
                ..StreamSettings::default()
            }],
            ..DeviceSettings::default()
        };
        match dev.apply(&retune) {
            Err(DeviceError::Unsupported(message)) => {
                assert!(message.contains("center_hz"), "{message}");
            }
            other => panic!("a per-stream centre on the quad must be Unsupported, got {other:?}"),
        }
        assert!(dev.settings().streams.is_empty());

        dev.apply(&DeviceSettings {
            streams: vec![StreamSettings {
                stream: 1,
                gains: vec![GainValue::new(GainKind::Tuner, 12.0)],
                ..StreamSettings::default()
            }],
            ..DeviceSettings::default()
        })
        .unwrap();
        let streams = &dev.settings().streams;
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].stream, 1);
        assert_eq!(
            streams[0].gains,
            vec![GainValue::new(GainKind::Tuner, 12.0)]
        );
    }

    #[test]
    fn transceiver_validates_a_per_stream_center_like_the_radio_wide_one() {
        let mut dev = open_virtual("transceiver");
        let out_of_range = DeviceSettings {
            streams: vec![StreamSettings {
                stream: 1,
                center_hz: Some(7_000_000_000.0),
                ..StreamSettings::default()
            }],
            ..DeviceSettings::default()
        };
        match dev.apply(&out_of_range) {
            Err(DeviceError::Unsupported(message)) => {
                assert!(message.contains("streams[1]"), "{message}");
            }
            other => panic!("an out-of-range lane centre must be Unsupported, got {other:?}"),
        }
        assert!(dev.settings().streams.is_empty());

        dev.apply(&DeviceSettings {
            streams: vec![StreamSettings {
                stream: 1,
                center_hz: Some(433_920_000.0),
                ..StreamSettings::default()
            }],
            ..DeviceSettings::default()
        })
        .unwrap();
        assert_eq!(dev.settings().streams[0].center_hz, Some(433_920_000.0));
    }

    #[test]
    fn a_per_stream_retune_moves_only_that_lanes_marker() {
        const N: usize = 1 << 17;
        const RETUNE_HZ: f64 = 50_000.0;

        let mut dev = open_virtual("transceiver");
        dev.apply(&DeviceSettings {
            streams: vec![StreamSettings {
                stream: 1,
                center_hz: Some(DEFAULT_CENTER_HZ - RETUNE_HZ),
                ..StreamSettings::default()
            }],
            ..DeviceSettings::default()
        })
        .unwrap();

        let mut receivers = Vec::new();
        let sinks = (0..2)
            .map(|_| {
                let (tx, rx) = mpsc::channel::<Vec<Complex<f32>>>();
                receivers.push(rx);
                RxSink::new(move |s, _| {
                    let _ = tx.send(s.to_vec());
                })
            })
            .collect();
        dev.rx_start(sinks).unwrap();
        let rate = dev.settings().sample_rate.unwrap();
        let lanes: Vec<Vec<Complex<f32>>> = receivers
            .iter()
            .map(|rx| {
                let mut samples = Vec::new();
                while samples.len() < N {
                    samples.extend(rx.recv_timeout(Duration::from_secs(2)).unwrap());
                }
                samples.truncate(N);
                samples
            })
            .collect();
        dev.rx_stop();

        let power = spectrum_of(&lanes[0]);
        let own = band_power(&power, rate, stream_marker_offset_hz(0), 10_000.0);
        let shifted = band_power(
            &power,
            rate,
            stream_marker_offset_hz(0) + RETUNE_HZ,
            10_000.0,
        );
        assert!(
            own > 100.0 * shifted,
            "lane 0 moved with lane 1's retune (own {own:.3e}, shifted {shifted:.3e})"
        );

        let power = spectrum_of(&lanes[1]);
        let moved = band_power(
            &power,
            rate,
            stream_marker_offset_hz(1) + RETUNE_HZ,
            10_000.0,
        );
        let old = band_power(&power, rate, stream_marker_offset_hz(1), 10_000.0);
        assert!(
            moved > 100.0 * old,
            "lane 1's marker did not follow its centre (moved {moved:.3e}, old {old:.3e})"
        );
    }

    #[test]
    fn a_marker_radio_takes_no_extra_settings() {
        for key in ["quad", "transceiver"] {
            let mut dev = open_virtual(key);
            assert!(dev.capabilities().extra.is_empty(), "{key}");
            assert_eq!(
                dev.capabilities().noise_source,
                sdrmm_wire::NoiseSource::None,
                "{key}"
            );
            let err = dev.apply(&DeviceSettings {
                extra: vec![sdrmm_wire::ExtraValue {
                    name: "bearing_deg".to_string(),
                    value: 10.0.into(),
                }],
                ..DeviceSettings::default()
            });
            assert!(
                matches!(err, Err(DeviceError::Unsupported(_))),
                "{key}: {err:?}"
            );
            assert!(
                matches!(dev.set_noise_source(true), Err(DeviceError::Unsupported(_))),
                "{key}"
            );
        }
    }

    #[test]
    fn marker_offsets_follow_each_lanes_own_center() {
        let caps = marker_capabilities(&MARKER_SHAPES[1]);
        let mut settings = default_settings();
        assert_eq!(marker_offsets(&settings, &caps), vec![50_000.0, 100_000.0]);

        settings.merge_from(&DeviceSettings {
            streams: vec![StreamSettings {
                stream: 1,
                center_hz: Some(DEFAULT_CENTER_HZ - 50_000.0),
                ..StreamSettings::default()
            }],
            ..DeviceSettings::default()
        });
        assert_eq!(marker_offsets(&settings, &caps), vec![50_000.0, 150_000.0]);

        settings.merge_from(&DeviceSettings {
            center_hz: Some(DEFAULT_CENTER_HZ + 25_000.0),
            ..DeviceSettings::default()
        });
        assert_eq!(marker_offsets(&settings, &caps), vec![50_000.0, 175_000.0]);

        let array = marker_capabilities(&MARKER_SHAPES[0]);
        assert_eq!(
            marker_offsets(&settings, &array),
            vec![50_000.0, 100_000.0, 150_000.0, 200_000.0]
        );
    }

    #[test]
    fn render_is_deterministic() {
        let a = render(2_048_000.0, 4096);
        let b = render(2_048_000.0, 4096);
        for (i, (x, y)) in a.iter().zip(&b).enumerate() {
            assert_eq!(x.re.to_bits(), y.re.to_bits(), "re mismatch at {i}");
            assert_eq!(x.im.to_bits(), y.im.to_bits(), "im mismatch at {i}");
        }
    }

    #[test]
    fn render_matches_the_streamed_band() {
        let n = 8192;
        let rendered = render(250_000.0, n);

        let mut dev = BandGen::new();
        dev.apply(&DeviceSettings {
            sample_rate: Some(250_000.0),
            ..DeviceSettings::default()
        })
        .unwrap();
        let (tx, rx) = mpsc::channel::<Vec<Complex<f32>>>();
        dev.rx_start(vec![RxSink::new(move |s, _| {
            let _ = tx.send(s.to_vec());
        })])
        .unwrap();
        let mut streamed = Vec::new();
        while streamed.len() < n {
            streamed.extend(rx.recv_timeout(Duration::from_secs(2)).unwrap());
        }
        dev.rx_stop();

        for (i, (a, b)) in rendered.iter().zip(&streamed).enumerate() {
            assert!(
                (a.re - b.re).abs() < 1e-4 && (a.im - b.im).abs() < 1e-4,
                "sample {i} diverged: {a} vs {b}"
            );
        }
    }

    #[test]
    fn rejects_unsupported_rate() {
        let mut dev = BandGen::new();
        let bad = DeviceSettings {
            sample_rate: Some(999.0),
            ..DeviceSettings::default()
        };
        assert!(dev.apply(&bad).is_err());
    }

    #[test]
    fn rejects_out_of_range_center() {
        let mut dev = BandGen::new();
        for bad in [7_000_000_000.0, -5_000_000_000.0, f64::NAN] {
            let err = dev.apply(&DeviceSettings {
                center_hz: Some(bad),
                ..DeviceSettings::default()
            });
            assert!(
                matches!(err, Err(DeviceError::Unsupported(_))),
                "center {bad} must be rejected"
            );
        }
        assert_eq!(dev.settings().center_hz, Some(100_000_000.0));
        dev.apply(&DeviceSettings {
            center_hz: Some(5_900_000_000.0),
            ..DeviceSettings::default()
        })
        .unwrap();
        assert_eq!(dev.settings().center_hz, Some(5_900_000_000.0));
    }

    #[test]
    fn apply_stores_every_field_for_state_round_trips() {
        let mut dev = BandGen::new();
        dev.apply(&DeviceSettings {
            ppm: Some(1.5),
            antenna: Some("RX".to_string()),
            bandwidth: Some(BandwidthSetting::Manual { hz: 1_500_000.0 }),
            gains: vec![GainValue::new(GainKind::Tuner, 16.0)],
            ..DeviceSettings::default()
        })
        .unwrap();

        let settings = dev.settings();
        assert_eq!(settings.ppm, Some(1.5));
        assert_eq!(settings.antenna.as_deref(), Some("RX"));
        assert_eq!(
            settings.bandwidth,
            Some(BandwidthSetting::Manual { hz: 1_500_000.0 })
        );
        assert_eq!(settings.gains.len(), 1);
        assert_eq!(settings.center_hz, Some(100_000_000.0));
    }

    #[test]
    fn streams_finite_samples_then_stops() {
        let mut dev = BandGen::new();
        dev.apply(&DeviceSettings {
            sample_rate: Some(250_000.0),
            ..DeviceSettings::default()
        })
        .unwrap();

        let (tx, rx) = mpsc::channel::<usize>();
        dev.rx_start(vec![RxSink::new(move |s, _| {
            let _ = tx.send(s.len());
        })])
        .unwrap();

        let mut total = 0;
        for _ in 0..3 {
            if let Ok(n) = rx.recv_timeout(Duration::from_secs(2)) {
                total += n;
            }
        }
        dev.rx_stop();
        assert!(total > 0, "expected streamed samples");

        dev.rx_start(vec![RxSink::new(|_, _| {})]).unwrap();
        dev.rx_stop();
    }

    #[test]
    fn output_is_finite_and_bounded() {
        let mut generator = Generator::new();
        let mut block = vec![Complex::new(0.0, 0.0); 4096];
        generator.fill(&mut block, 2_048_000.0);
        for s in &block {
            assert!(s.re.is_finite() && s.im.is_finite());
            assert!(s.norm() < 2.0, "unexpectedly large sample");
        }
    }
}

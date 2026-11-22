//! A radio that hears one tone: the smallest complete device plugin. Copy this crate, swap the
//! tone for your hardware, build it with `cargo build --release` and drop the library into the
//! SDRmm plugins folder.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use num_complex::Complex;
use sdrmm_device_plugin::{
    DeviceDriver, DeviceError, RxSink, SdrDevice,
    device::{Sample, single_rx_sink},
    export_driver,
    wire::{BackendReach, BackendSpec, Capabilities, DeviceBackend, DeviceInfo, DeviceSettings},
};

const TONE: DeviceBackend = DeviceBackend::new("tone");
const SAMPLE_RATE: f64 = 250_000.0;
const TONE_HZ: f64 = 10_000.0;
const BLOCK: usize = 4_096;

#[derive(Default)]
pub struct ToneDriver;

impl DeviceDriver for ToneDriver {
    fn id(&self) -> DeviceBackend {
        TONE
    }

    fn spec(&self) -> BackendSpec {
        BackendSpec::new(TONE, "Tone", BackendReach::Local)
    }

    fn probe(&self) -> Vec<DeviceInfo> {
        vec![DeviceInfo {
            driver: TONE,
            key: "0".to_owned(),
            label: "Tone".to_owned(),
            serial: None,
            profile: None,
        }]
    }

    fn open(&self, _info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        Ok(Box::new(ToneDevice::new()?))
    }
}

struct ToneDevice {
    capabilities: Capabilities,
    settings: DeviceSettings,
    running: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl ToneDevice {
    fn new() -> Result<Self, DeviceError> {
        let capabilities = serde_json::from_value(serde_json::json!({
            "freq_ranges": [{ "min": 0.0, "max": 6e9 }],
            "sample_rates": [SAMPLE_RATE],
            "gains": [],
            "antennas": [],
            "bandwidths": [],
        }))
        .map_err(|error| DeviceError::Io(format!("tone capabilities: {error}")))?;
        Ok(Self {
            capabilities,
            settings: DeviceSettings {
                center_hz: Some(100e6),
                sample_rate: Some(SAMPLE_RATE),
                ..DeviceSettings::default()
            },
            running: Arc::new(AtomicBool::new(false)),
            worker: None,
        })
    }
}

fn stream(mut sink: RxSink, running: &AtomicBool) {
    let step = std::f64::consts::TAU * TONE_HZ / SAMPLE_RATE;
    let pace = Duration::from_secs_f64(BLOCK as f64 / SAMPLE_RATE);
    let mut block = vec![Sample::new(0.0, 0.0); BLOCK];
    let mut phase = 0.0_f64;
    while running.load(Ordering::Acquire) {
        for sample in &mut block {
            *sample = Complex::new(phase.cos() as f32, phase.sin() as f32) * 0.5;
            phase = (phase + step) % std::f64::consts::TAU;
        }
        sink.push(&block);
        std::thread::sleep(pace);
    }
}

impl SdrDevice for ToneDevice {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn settings(&self) -> &DeviceSettings {
        &self.settings
    }

    fn apply(&mut self, settings: &DeviceSettings) -> Result<(), DeviceError> {
        if let Some(rate) = settings.sample_rate.filter(|rate| *rate != SAMPLE_RATE) {
            return Err(DeviceError::Unsupported(format!("sample_rate {rate}")));
        }
        if let Some(center_hz) = settings.center_hz {
            self.settings.center_hz = Some(center_hz);
        }
        Ok(())
    }

    fn rx_start(&mut self, sinks: Vec<RxSink>) -> Result<(), DeviceError> {
        if self.worker.is_some() {
            return Err(DeviceError::AlreadyStreaming);
        }
        let sink = single_rx_sink(sinks)?;
        self.running.store(true, Ordering::Release);
        let running = self.running.clone();
        let worker = std::thread::Builder::new()
            .name("tone".to_owned())
            .spawn(move || stream(sink, &running))
            .map_err(|error| DeviceError::Io(format!("tone thread: {error}")))?;
        self.worker = Some(worker);
        tracing::info!("tone streaming");
        Ok(())
    }

    fn rx_stop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            tracing::warn!("tone thread panicked");
        }
    }
}

impl Drop for ToneDevice {
    fn drop(&mut self) {
        self.rx_stop();
    }
}

export_driver!(ToneDriver);

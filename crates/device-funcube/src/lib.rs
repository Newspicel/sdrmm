#![cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]

use std::sync::{Arc, Mutex, MutexGuard};

use sdrmm_device::{
    Capture, CaptureConfig, CaptureRadio, DeviceDriver, DeviceError, Direction, DuplexState,
    RxSink, SdrDevice, lock, single_rx_sink,
};
use sdrmm_wire::{Capabilities, DeviceInfo, DeviceSettings};

use crate::{
    caps::Writes, convert::FunCubeConverter, error::Error, protocol::SAMPLE_RATE_HZ, radio::FunCube,
};

mod caps;
mod convert;
mod discovery;
mod error;
#[cfg(test)]
mod hardware;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
mod protocol;
mod radio;
#[cfg(any(test, target_os = "linux", target_os = "windows"))]
mod raw;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

const DRIVER_ID: &str = "funcube";
const LABEL: &str = "FUNcube Dongle Pro+";

fn map_err(err: Error) -> DeviceError {
    let text = err.to_string();
    if err.is_disconnected() {
        return DeviceError::Disconnected(text);
    }
    if err.is_busy() {
        return DeviceError::InUse(text);
    }
    if err.is_permission_denied() {
        return DeviceError::PermissionDenied(text);
    }
    match err {
        Error::DeviceNotFound => DeviceError::NotFound(text),
        #[cfg(target_os = "macos")]
        Error::AudioNotFound => DeviceError::NotFound(text),
        Error::InvalidConfig { .. } => DeviceError::Unsupported(text),
        _ => DeviceError::Io(text),
    }
}

fn device_info(key: String) -> DeviceInfo {
    DeviceInfo {
        driver: DRIVER_ID.to_string(),
        key,
        label: LABEL.to_string(),
        serial: None,
        profile: None,
    }
}

#[derive(Default)]
pub struct FunCubeDriver;

impl FunCubeDriver {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn open_device(&self, info: &DeviceInfo) -> Result<FunCubeDevice, DeviceError> {
        let (control, port) = platform::open(&info.key).map_err(map_err)?;
        let radio = FunCube::open(control).map_err(map_err)?;
        Ok(FunCubeDevice::new(radio, port, &info.key))
    }
}

impl DeviceDriver for FunCubeDriver {
    fn id(&self) -> &'static str {
        DRIVER_ID
    }

    fn probe(&self) -> Vec<DeviceInfo> {
        match discovery::list() {
            Ok(found) => found
                .iter()
                .map(|info| device_info(discovery::key(info)))
                .collect(),
            Err(e) => {
                tracing::warn!("funcube enumerate failed: {e}");
                Vec::new()
            }
        }
    }

    fn open(&self, info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        Ok(Box::new(self.open_device(info)?))
    }
}

struct FunCubeRadio {
    device: Mutex<FunCube<platform::Control>>,
    port: platform::Port,
}

impl FunCubeRadio {
    fn lock(&self) -> MutexGuard<'_, FunCube<platform::Control>> {
        lock(&self.device)
    }
}

impl CaptureRadio for FunCubeRadio {
    type Stream = platform::Stream;

    fn arm(&self) -> Result<platform::Stream, DeviceError> {
        self.port.start().map_err(map_err)
    }
}

pub struct FunCubeDevice {
    radio: Arc<FunCubeRadio>,
    capabilities: Capabilities,
    settings: DeviceSettings,
    duplex: Arc<Mutex<DuplexState>>,
    capture: Capture<FunCubeRadio>,
}

impl FunCubeDevice {
    fn new(radio: FunCube<platform::Control>, port: platform::Port, key: &str) -> Self {
        let capabilities = caps::capabilities();
        let settings = caps::settings(radio.config());
        tracing::info!(
            firmware = radio.firmware(),
            key,
            "opened funcube dongle pro+"
        );
        Self {
            radio: Arc::new(FunCubeRadio {
                device: Mutex::new(radio),
                port,
            }),
            duplex: Arc::new(Mutex::new(DuplexState::new(capabilities.duplex))),
            capabilities,
            settings,
            capture: Capture::new(),
        }
    }
}

fn write_to_hardware(radio: &mut FunCube<platform::Control>, writes: &Writes) -> Result<(), Error> {
    if let Some(ppm) = writes.ppm {
        radio.set_ppm(ppm)?;
    }
    if let Some(hz) = writes.center_hz {
        radio.set_frequency_hz(hz)?;
    }
    if let Some(filter) = writes.rf_filter {
        radio.set_rf_filter(filter)?;
    }
    if let Some(on) = writes.lna {
        radio.set_lna(on)?;
    }
    if let Some(on) = writes.mixer {
        radio.set_mixer(on)?;
    }
    if let Some(db) = writes.if_gain_db {
        radio.set_if_gain_db(db)?;
    }
    if let Some(filter) = writes.if_filter {
        radio.set_if_filter(filter)?;
    }
    if let Some(on) = writes.bias_tee {
        radio.set_bias_tee(on)?;
    }
    Ok(())
}

impl SdrDevice for FunCubeDevice {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn settings(&self) -> &DeviceSettings {
        &self.settings
    }

    fn apply(&mut self, settings: &DeviceSettings) -> Result<(), DeviceError> {
        let writes = caps::plan(settings, &self.capabilities)?;
        let (result, config) = {
            let mut radio = self.radio.lock();
            let result = write_to_hardware(&mut radio, &writes);
            (result, radio.config().clone())
        };
        self.settings = caps::settings(&config);
        result.map_err(map_err)
    }

    fn rx_start(&mut self, sinks: Vec<RxSink>) -> Result<(), DeviceError> {
        let sink = single_rx_sink(sinks)?;
        lock(&self.duplex).claim(Direction::Rx)?;
        let started = self.capture.start(
            self.radio.clone(),
            FunCubeConverter::new(platform::FORMAT, platform::BLOCK_SAMPLES),
            sink,
            CaptureConfig::new("sdrmm-funcube-rx", DRIVER_ID)
                .with_sample_rate(Some(f64::from(SAMPLE_RATE_HZ))),
        );
        if started.is_err() {
            lock(&self.duplex).release(Direction::Rx);
        }
        started
    }

    fn rx_stop(&mut self) {
        self.capture.stop();
        lock(&self.duplex).release(Direction::Rx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dongle_is_named_by_the_port_it_is_plugged_into() {
        let info = device_info("1-4".to_string());
        assert_eq!(info.driver, "funcube");
        assert_eq!(info.key, "1-4");
        assert_eq!(info.label, "FUNcube Dongle Pro+");
        assert_eq!(info.serial, None);
    }

    #[test]
    fn errors_map_onto_what_the_engine_acts_on() {
        assert!(matches!(
            map_err(Error::DeviceNotFound),
            DeviceError::NotFound(_)
        ));
        assert!(matches!(
            map_err(Error::invalid_config("frequency", "x")),
            DeviceError::Unsupported(_)
        ));
        assert!(matches!(
            map_err(Error::protocol("start", "x")),
            DeviceError::Io(_)
        ));
    }
}

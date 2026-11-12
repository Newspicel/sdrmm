use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use sdrmm_device::{
    Capture, CaptureConfig, DeviceDriver, DeviceError, Direction, DuplexState, RxSink, SdrDevice,
    TxStream, lock, single_rx_sink,
};
use sdrmm_wire::{
    Capabilities, CheckStatus, DeviceBackend, DeviceInfo, DeviceSettings, DoctorCheck, Readout,
};

mod api;
mod caps;
mod ffi;
mod link;
mod model;
mod settings;
mod stream;
#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;
mod tx;

pub use api::{OVERRIDE_VAR, Rtsa, shared};
use caps::{CENTER, Profile, REFERENCE, rx_scale};
use link::Link;
pub use model::DRIVER_ID;
use model::{Found, Kind, Mode};
use stream::{FloatConverter, Gauge, RxRadio};
use tx::{AaroniaTx, Tune};

const STARTUP_SAMPLE_RATE_HZ: f64 = 2_000_000.0;
const STARTUP_REFERENCE_DBM: f64 = -20.0;
const PROBE_RESCAN: Duration = Duration::from_millis(250);
const DEEP_RESCAN: Duration = Duration::from_secs(2);
const TX_START_LIMIT: Duration = Duration::from_secs(10);
const TX_START_POLL: Duration = Duration::from_millis(50);

const HEALTH: [(&str, &str, &str, &str); 3] = [
    ("fpgatemp", "fpga_temp", "FPGA", "°C"),
    ("frontendtemp", "frontend_temp", "Front end", "°C"),
    ("usboverflowssecond", "usb_overflows", "USB overflows", "/s"),
];

#[derive(Clone, Debug)]
pub struct RuntimeInfo {
    pub version: Option<u32>,
    pub library: Option<String>,
    pub error: Option<String>,
}

#[must_use]
pub fn runtime_info() -> RuntimeInfo {
    match shared() {
        Ok(api) => RuntimeInfo {
            version: Some(api.version()),
            library: Some(api.library_path().display().to_string()),
            error: None,
        },
        Err(error) => RuntimeInfo {
            version: None,
            library: None,
            error: Some(error),
        },
    }
}

#[must_use]
pub fn version_text(version: u32) -> String {
    format!("{}.{}", version >> 16, version & 0xffff)
}

#[must_use]
pub fn health(info: &RuntimeInfo) -> DoctorCheck {
    let loaded = info
        .library
        .as_ref()
        .zip(info.version)
        .map(|(library, version)| format!("version {}\nlibrary: {library}", version_text(version)));
    let installed = loaded.is_some();
    DoctorCheck {
        id: "aaronia.api".to_string(),
        name: "Aaronia RTSA API".to_string(),
        status: if installed {
            CheckStatus::Ok
        } else {
            CheckStatus::Warn
        },
        detail: loaded.unwrap_or_else(|| {
            info.error
                .clone()
                .unwrap_or_else(|| "not installed".to_string())
        }),
        hint: (!installed).then(|| {
            format!(
                "SPECTRAN V6 radios need the Aaronia RTSA-Suite PRO from \
                 https://aaronia.com/en/support/downloads, which is not part of this package. \
                 Set {OVERRIDE_VAR} to its folder if it is installed elsewhere."
            )
        }),
    }
}

#[derive(Default)]
pub struct AaroniaDriver {
    api: Option<Arc<dyn Rtsa>>,
}

impl AaroniaDriver {
    #[must_use]
    pub fn new() -> Self {
        Self { api: None }
    }

    #[must_use]
    pub fn with_api(api: Arc<dyn Rtsa>) -> Self {
        Self { api: Some(api) }
    }

    fn api(&self) -> Option<Arc<dyn Rtsa>> {
        match &self.api {
            Some(api) => Some(api.clone()),
            None => shared().ok(),
        }
    }

    /// Rescans only while this process has nothing open, so a search never disturbs a stream.
    fn list(&self, rescan: Duration) -> Vec<Found> {
        let Some(api) = self.api() else {
            return Vec::new();
        };
        let rescan = (api.open_count() == 0).then_some(rescan);
        match api.devices(rescan) {
            Ok(found) => found,
            Err(error) => {
                tracing::warn!("SPECTRAN enumerate failed: {error}");
                Vec::new()
            }
        }
    }
}

impl DeviceDriver for AaroniaDriver {
    fn id(&self) -> DeviceBackend {
        DRIVER_ID
    }

    fn health(&self) -> Option<DoctorCheck> {
        if self.api.is_none() && !api::platform_supported() {
            return None;
        }
        Some(health(&runtime_info()))
    }

    fn probe(&self) -> Vec<DeviceInfo> {
        self.list(PROBE_RESCAN)
            .iter()
            .map(Found::describe)
            .collect()
    }

    fn probe_deep(&self) -> Vec<DeviceInfo> {
        self.list(DEEP_RESCAN).iter().map(Found::describe).collect()
    }

    fn open(&self, info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        let api = self.api().ok_or_else(|| DeviceError::NotFound(info.id()))?;
        let found = self
            .list(DEEP_RESCAN)
            .into_iter()
            .find(|found| found.serial == info.key)
            .ok_or_else(|| DeviceError::NotFound(info.id()))?;
        Ok(Box::new(AaroniaDevice::open(
            api,
            found.kind,
            &found.serial,
        )?))
    }
}

pub struct AaroniaDevice {
    link: Arc<Link>,
    profile: Profile,
    capabilities: Capabilities,
    settings: DeviceSettings,
    gauge: Arc<Gauge>,
    tune: Arc<Tune>,
    duplex: Arc<Mutex<DuplexState>>,
    radio: Arc<RxRadio>,
    capture: Capture<RxRadio>,
}

impl AaroniaDevice {
    fn open(api: Arc<dyn Rtsa>, kind: Kind, serial: &str) -> Result<Self, DeviceError> {
        let tx = Self::transmitter_profile(&api, kind, serial)?;
        let link = Arc::new(Link::open(api.clone(), kind, serial, Mode::Receiver)?);
        let dev = link.handle_in(Mode::Receiver)?;
        let mut profile = Profile::read_rx(api.as_ref(), dev, kind)?;
        profile.tx = tx;
        let capabilities = profile.capabilities();
        let gauge = Arc::new(Gauge::default());
        let mut this = Self {
            radio: Arc::new(RxRadio::new(link.clone(), gauge.clone())),
            link,
            profile,
            duplex: Arc::new(Mutex::new(DuplexState::new(capabilities.duplex))),
            capabilities,
            settings: DeviceSettings::default(),
            gauge,
            tune: Arc::new(Tune::default()),
            capture: Capture::new(),
        };
        let start = this.startup(dev);
        this.apply(&start)?;
        Ok(this)
    }

    /// The transmitter tree is only readable with the device open as a transmitter, so it is
    /// opened that way once to learn it. A SPECTRAN that refuses is a receiver.
    fn transmitter_profile(
        api: &Arc<dyn Rtsa>,
        kind: Kind,
        serial: &str,
    ) -> Result<Option<caps::TxProfile>, DeviceError> {
        let dev = match api.open(kind, Mode::Transmitter, serial) {
            Ok(dev) => dev,
            Err(DeviceError::InUse(reason)) => return Err(DeviceError::InUse(reason)),
            Err(error) => {
                tracing::info!(%error, "this SPECTRAN does not open as a transmitter");
                return Ok(None);
            }
        };
        let profile = Profile::read_tx(api.as_ref(), dev);
        api.close(dev)?;
        profile.map(Some)
    }

    fn startup(&self, dev: api::DevHandle) -> DeviceSettings {
        let api = &self.link.api;
        let rates = self.profile.rate_range();
        let reference = api
            .get_float(dev, REFERENCE)
            .unwrap_or(STARTUP_REFERENCE_DBM);
        DeviceSettings {
            center_hz: api.get_float(dev, CENTER).ok(),
            sample_rate: Some(STARTUP_SAMPLE_RATE_HZ.clamp(rates.min, rates.max)),
            antenna: self.capabilities.antennas.first().cloned(),
            gains: vec![sdrmm_wire::GainValue {
                stage: caps::GAIN.to_string(),
                value_db: -reference,
            }],
            ..DeviceSettings::default()
        }
    }

    fn refresh(&self) {
        let reference = self
            .settings
            .gain(caps::GAIN)
            .map_or(STARTUP_REFERENCE_DBM, caps::reference_for_gain);
        let rate = self.settings.sample_rate.unwrap_or(STARTUP_SAMPLE_RATE_HZ);
        self.gauge.set(rx_scale(reference), rate);
        self.tune
            .set(self.settings.center_hz.unwrap_or_default(), rate);
    }

    /// Opens the device in `mode` and, when that meant reopening it, replays every setting.
    fn enter(&mut self, mode: Mode) -> Result<api::DevHandle, DeviceError> {
        let (dev, reopened) = self.link.switch(mode)?;
        if reopened {
            let replay = settings::replay(&self.settings);
            settings::push(self.link.api.as_ref(), dev, mode, &self.profile, &replay)?;
        }
        Ok(dev)
    }

    fn start_transmitter(&mut self) -> Result<api::DevHandle, DeviceError> {
        let dev = self.enter(Mode::Transmitter)?;
        let api = &self.link.api;
        api.connect(dev)?;
        let started = api.start(dev).and_then(|()| {
            let deadline = Instant::now() + TX_START_LIMIT;
            while !api.running(dev) {
                if Instant::now() >= deadline {
                    return Err(DeviceError::Io(
                        "the SPECTRAN transmitter did not come up".to_string(),
                    ));
                }
                std::thread::sleep(TX_START_POLL);
            }
            Ok(())
        });
        if let Err(error) = started {
            if let Err(cleanup) = api.stop(dev).and(api.disconnect(dev)) {
                tracing::warn!(%cleanup, "releasing a SPECTRAN transmitter that did not start");
            }
            return Err(error);
        }
        Ok(dev)
    }

    fn check_tx_rate(&self) -> Result<(), DeviceError> {
        let rate = self.tune.rate();
        match self.profile.tx_span_max() {
            None => Err(DeviceError::Unsupported(
                "this SPECTRAN does not transmit".to_string(),
            )),
            Some(max) if rate > max => Err(DeviceError::Unsupported(format!(
                "the SPECTRAN transmits at most {max} Hz wide, not {rate} Hz"
            ))),
            Some(_) => Ok(()),
        }
    }
}

impl SdrDevice for AaroniaDevice {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn settings(&self) -> &DeviceSettings {
        &self.settings
    }

    fn apply(&mut self, delta: &DeviceSettings) -> Result<(), DeviceError> {
        settings::validate(delta, &self.capabilities)?;
        if let Some((mode, dev)) = self.link.current() {
            let api = self.link.api.as_ref();
            settings::push(api, dev, mode, &self.profile, delta)?;
            self.settings.merge_from(delta);
            if mode == Mode::Receiver {
                let actual = settings::read_receiver(api, dev)?;
                self.settings.merge_from(&actual);
            }
        } else {
            self.settings.merge_from(delta);
        }
        self.refresh();
        Ok(())
    }

    fn rx_start(&mut self, sinks: Vec<RxSink>) -> Result<(), DeviceError> {
        let sink = single_rx_sink(sinks)?;
        lock(&self.duplex).claim(Direction::Rx)?;
        let started = self.enter(Mode::Receiver).and_then(|dev| {
            let actual = settings::read_receiver(self.link.api.as_ref(), dev)?;
            self.settings.merge_from(&actual);
            self.refresh();
            let config = CaptureConfig::new("sdrmm-aaronia-rx", "SPECTRAN")
                .with_sample_rate(self.settings.sample_rate);
            self.capture.start(
                self.radio.clone(),
                FloatConverter::new(self.gauge.clone()),
                sink,
                config,
            )
        });
        if started.is_err() {
            lock(&self.duplex).release(Direction::Rx);
        }
        started
    }

    fn rx_stop(&mut self) {
        self.capture.stop();
        lock(&self.duplex).release(Direction::Rx);
    }

    fn tx_start_channels(&mut self, channels: &[u32]) -> Result<Box<dyn TxStream>, DeviceError> {
        if channels != [0] {
            return Err(DeviceError::Unsupported(format!(
                "this device has 1 tx stream, got channels {channels:?}"
            )));
        }
        self.check_tx_rate()?;
        lock(&self.duplex).claim(Direction::Tx)?;
        match self.start_transmitter() {
            Ok(dev) => Ok(Box::new(AaroniaTx::new(
                self.link.clone(),
                dev,
                self.duplex.clone(),
                self.tune.clone(),
            ))),
            Err(error) => {
                lock(&self.duplex).release(Direction::Tx);
                Err(error)
            }
        }
    }

    fn readouts(&self) -> Result<Vec<Readout>, DeviceError> {
        if lock(&self.duplex).is_idle() {
            return Ok(Vec::new());
        }
        let Some((_, dev)) = self.link.current() else {
            return Ok(Vec::new());
        };
        let mut readouts = Vec::new();
        for (path, name, label, unit) in HEALTH {
            if let Some(value) = self.link.api.health(dev, path)? {
                readouts.push(Readout::number(name, label, value, unit));
            }
        }
        Ok(readouts)
    }
}

impl Drop for AaroniaDevice {
    fn drop(&mut self) {
        self.capture.stop();
    }
}

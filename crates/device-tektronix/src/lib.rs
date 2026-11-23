use std::sync::Arc;

use sdrmm_device::{
    Capture, CaptureConfig, DeviceDriver, DeviceError, RxSink, SdrDevice, single_rx_sink,
};
use sdrmm_wire::{
    BackendReach, BackendSpec, Capabilities, CheckStatus, DeviceBackend, DeviceInfo,
    DeviceSettings, DoctorCheck, ExtraValue, GainValue, UsbIdentity,
};

mod api;
mod caps;
mod ffi;
#[cfg(test)]
mod hardware;
mod link;
mod model;
mod settings;
mod stream;
#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;

pub use api::{OVERRIDE_VAR, Rsa, shared};
use caps::{AUTO_ATTENUATION, GAIN, INPUT, reference_for_gain, rx_scale};
use link::Link;
pub use model::DRIVER_ID;
use model::Found;
use stream::{FloatConverter, Gauge, RxRadio};

const STARTUP_SAMPLE_RATE_HZ: f64 = 1_750_000.0;
const STARTUP_REFERENCE_DBM: f64 = -20.0;
const DOWNLOAD: &str = "https://www.tek.com/en/support/software/application/rsa-application-programming-interface-api-for-64bit-windows-v560295";

#[derive(Clone, Debug)]
pub struct RuntimeInfo {
    pub version: Option<String>,
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
pub fn health(info: &RuntimeInfo) -> DoctorCheck {
    let loaded = info
        .library
        .as_ref()
        .zip(info.version.as_ref())
        .map(|(library, version)| format!("version {version}\nlibrary: {library}"));
    let installed = loaded.is_some();
    DoctorCheck {
        id: "tektronix.api".to_string(),
        name: "Tektronix RSA API".to_string(),
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
                "Tektronix RSA analysers need the RSA API from {DOWNLOAD}, which is not part of \
                 this package. Set {OVERRIDE_VAR} to its folder if it is installed elsewhere."
            )
        }),
    }
}

#[derive(Default)]
pub struct TektronixDriver {
    api: Option<Arc<dyn Rsa>>,
}

impl TektronixDriver {
    #[must_use]
    pub fn new() -> Self {
        Self { api: None }
    }

    #[must_use]
    pub fn with_api(api: Arc<dyn Rsa>) -> Self {
        Self { api: Some(api) }
    }

    fn api(&self) -> Option<Arc<dyn Rsa>> {
        match &self.api {
            Some(api) => Some(api.clone()),
            None => shared().ok(),
        }
    }

    fn list(&self) -> Vec<Found> {
        let Some(api) = self.api() else {
            return Vec::new();
        };
        match api.devices() {
            Ok(found) => found,
            Err(error) => {
                tracing::warn!("Tektronix analyser search failed: {error}");
                Vec::new()
            }
        }
    }
}

impl DeviceDriver for TektronixDriver {
    fn id(&self) -> DeviceBackend {
        DRIVER_ID
    }

    fn spec(&self) -> BackendSpec {
        BackendSpec::new(DRIVER_ID, "Tektronix RSA", BackendReach::Local).usb([
            UsbIdentity::new(0x0699, Some(0xa306), "Tektronix RSA306"),
            UsbIdentity::new(0x0699, Some(0xa500), "Tektronix RSA500/600"),
        ])
    }

    fn health(&self) -> Option<DoctorCheck> {
        if self.api.is_none() && !api::platform_supported() {
            return None;
        }
        Some(health(&runtime_info()))
    }

    fn probe(&self) -> Vec<DeviceInfo> {
        self.list().iter().map(Found::describe).collect()
    }

    fn open(&self, info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        let api = self.api().ok_or_else(|| DeviceError::NotFound(info.id()))?;
        let found = self
            .list()
            .into_iter()
            .find(|found| found.serial == info.key)
            .ok_or_else(|| DeviceError::NotFound(info.id()))?;
        Ok(Box::new(TektronixDevice::open(api, &found)?))
    }
}

pub struct TektronixDevice {
    link: Arc<Link>,
    capabilities: Capabilities,
    settings: DeviceSettings,
    gauge: Arc<Gauge>,
    radio: Arc<RxRadio>,
    capture: Capture<RxRadio>,
}

impl TektronixDevice {
    fn open(api: Arc<dyn Rsa>, found: &Found) -> Result<Self, DeviceError> {
        let link = Arc::new(Link::connect(api, found)?);
        let gauge = Arc::new(Gauge::default());
        let mut this = Self {
            radio: Arc::new(RxRadio::new(link.clone(), gauge.clone())),
            capabilities: caps::capabilities(&link.limits, link.fitted),
            link,
            settings: DeviceSettings {
                antenna: Some(INPUT.to_string()),
                ..DeviceSettings::default()
            },
            gauge,
            capture: Capture::new(),
        };
        let start = this.startup()?;
        this.apply(&start)?;
        Ok(this)
    }

    fn startup(&self) -> Result<DeviceSettings, DeviceError> {
        let api = &self.link.api;
        let rates = &self.capabilities.sample_rates;
        let rate = rates
            .iter()
            .copied()
            .find(|rate| *rate >= STARTUP_SAMPLE_RATE_HZ)
            .or(rates.last().copied());
        Ok(DeviceSettings {
            center_hz: Some(api.center()?),
            sample_rate: rate,
            gains: vec![GainValue {
                stage: GAIN.to_string(),
                value_db: -STARTUP_REFERENCE_DBM,
            }],
            extra: self
                .link
                .fitted
                .attenuator
                .then(|| ExtraValue {
                    name: AUTO_ATTENUATION.to_string(),
                    value: true.into(),
                })
                .into_iter()
                .collect(),
            ..DeviceSettings::default()
        })
    }

    fn refresh(&self) {
        let reference = self
            .settings
            .gain(GAIN)
            .map_or(STARTUP_REFERENCE_DBM, reference_for_gain);
        let rate = self.settings.sample_rate.unwrap_or(STARTUP_SAMPLE_RATE_HZ);
        self.gauge.set(rx_scale(reference), rate);
    }
}

impl SdrDevice for TektronixDevice {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn settings(&self) -> &DeviceSettings {
        &self.settings
    }

    fn apply(&mut self, delta: &DeviceSettings) -> Result<(), DeviceError> {
        settings::validate(delta, &self.capabilities)?;
        let changed = settings::changes(delta, &self.settings);
        settings::push(&self.link, &changed)?;
        self.settings.merge_from(&changed);
        let actual = settings::read_back(&self.link)?;
        self.settings.merge_from(&actual);
        self.refresh();
        Ok(())
    }

    fn rx_start(&mut self, sinks: Vec<RxSink>) -> Result<(), DeviceError> {
        let sink = single_rx_sink(sinks)?;
        self.refresh();
        let config = CaptureConfig::new("sdrmm-tektronix-rx", "Tektronix RSA")
            .with_sample_rate(self.settings.sample_rate);
        self.capture.start(
            self.radio.clone(),
            FloatConverter::new(self.gauge.clone()),
            sink,
            config,
        )
    }

    fn rx_stop(&mut self) {
        self.capture.stop();
    }
}

impl Drop for TektronixDevice {
    fn drop(&mut self) {
        self.capture.stop();
    }
}

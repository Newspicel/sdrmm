use std::{sync::Mutex, time::Duration};

use sdrmm_device::{
    DeviceDriver, DeviceError, Duplex, RxSink, Sample, SdrDevice, SweepPlan, SweepSink, TxStream,
    lock,
};
use sdrmm_wire::{
    AgcGain, BackendSpec, Capabilities, CheckStatus, DeviceBackend, DeviceInfo, DeviceSettings,
    DoctorCheck, Readout,
};

use super::{json, sink, status};
use crate::abi::{Handle, MAX_TX_CHANNELS, PluginV1, SamplesV1, Str, SweepPlanV1};

/// A plugin's driver, seen by the registry like any built-in. The last call that failed is kept
/// for the doctor and logged once, so a plugin that fails on every discovery tick neither hides
/// nor floods the log.
pub struct PluginDriver {
    table: &'static PluginV1,
    spec: BackendSpec,
    failure: Mutex<Option<(&'static str, String)>>,
}

impl PluginDriver {
    pub(crate) fn new(table: &'static PluginV1, spec: BackendSpec) -> Self {
        Self {
            table,
            spec,
            failure: Mutex::new(None),
        }
    }

    /// Keeps `call`'s failure until the same call succeeds again.
    fn settle<T>(&self, call: &'static str, result: Result<T, DeviceError>, fallback: T) -> T {
        let mut failure = lock(&self.failure);
        match result {
            Ok(value) => {
                if failure.as_ref().is_some_and(|(failed, _)| *failed == call) {
                    *failure = None;
                }
                value
            }
            Err(error) => {
                let text = format!("{call} failed: {error}");
                if failure.as_ref().is_none_or(|(_, known)| *known != text) {
                    tracing::warn!(backend = %self.spec.backend, "device plugin {text}");
                    *failure = Some((call, text));
                }
                fallback
            }
        }
    }

    fn listed(&self, deep: bool) -> Vec<DeviceInfo> {
        let table = self.table;
        let found = json(table, |out| (table.probe)(table.driver, deep, out));
        self.settle("probe", found, Vec::new())
    }
}

impl Drop for PluginDriver {
    fn drop(&mut self) {
        (self.table.driver_drop)(self.table.driver);
    }
}

impl DeviceDriver for PluginDriver {
    fn id(&self) -> DeviceBackend {
        self.spec.backend
    }

    fn spec(&self) -> BackendSpec {
        self.spec.clone()
    }

    fn probe(&self) -> Vec<DeviceInfo> {
        self.listed(false)
    }

    fn probe_deep(&self) -> Vec<DeviceInfo> {
        self.listed(true)
    }

    fn open(&self, info: &DeviceInfo) -> Result<Box<dyn SdrDevice>, DeviceError> {
        let table = self.table;
        let info = serde_json::to_vec(info)
            .map_err(|error| DeviceError::Io(format!("cannot encode the device: {error}")))?;
        let mut handle: Handle = std::ptr::null_mut();
        status(
            table,
            (table.open)(table.driver, Str::of(&info), &raw mut handle),
        )?;
        PluginDevice::adopt(table, handle).map(|device| Box::new(device) as Box<dyn SdrDevice>)
    }

    fn resolve(&self, key: &str) -> Option<DeviceInfo> {
        let table = self.table;
        let found = json(table, |out| {
            (table.resolve)(table.driver, Str::of(key.as_bytes()), out)
        });
        self.settle("resolve", found, None)
    }

    fn health(&self) -> Option<DoctorCheck> {
        let table = self.table;
        let own = json(table, |out| (table.health)(table.driver, out));
        let own = self.settle("health", own, None);
        match lock(&self.failure).clone() {
            Some((_, detail)) => Some(DoctorCheck {
                id: format!("plugins.device.{}.failure", self.spec.backend),
                name: format!("{} plugin", self.spec.label),
                status: CheckStatus::Fail,
                detail,
                hint: Some("the plugin's own log lines say more".to_owned()),
            }),
            None => own,
        }
    }

    fn write_serial(&self, key: &str, serial: Option<&str>) -> Result<String, DeviceError> {
        let table = self.table;
        let serial = serde_json::to_vec(&serial)
            .map_err(|error| DeviceError::Io(format!("cannot encode the serial: {error}")))?;
        json(table, |out| {
            (table.write_serial)(table.driver, Str::of(key.as_bytes()), Str::of(&serial), out)
        })
    }
}

/// An open plugin radio. Capabilities and settings are copied after every call that may change
/// them, since the trait lends them by reference.
struct PluginDevice {
    table: &'static PluginV1,
    handle: Handle,
    capabilities: Capabilities,
    settings: DeviceSettings,
}

/// A plugin device is `Send` like the `SdrDevice` it wraps, and the host only calls it from the
/// thread holding it.
unsafe impl Send for PluginDevice {}

impl PluginDevice {
    fn adopt(table: &'static PluginV1, handle: Handle) -> Result<Self, DeviceError> {
        let capabilities = match json(table, |out| (table.capabilities)(handle, out)) {
            Ok(capabilities) => capabilities,
            Err(error) => {
                (table.device_drop)(handle);
                return Err(error);
            }
        };
        let mut device = Self {
            table,
            handle,
            capabilities,
            settings: DeviceSettings::default(),
        };
        device.refresh()?;
        Ok(device)
    }

    fn refresh(&mut self) -> Result<(), DeviceError> {
        let (table, handle) = (self.table, self.handle);
        self.capabilities = json(table, |out| (table.capabilities)(handle, out))?;
        self.settings = json(table, |out| (table.settings)(handle, out))?;
        Ok(())
    }

    fn call(
        &mut self,
        call: impl FnOnce(&PluginV1, Handle) -> crate::abi::Status,
    ) -> Result<(), DeviceError> {
        let result = status(self.table, call(self.table, self.handle));
        let refreshed = self.refresh();
        result?;
        refreshed
    }
}

impl Drop for PluginDevice {
    fn drop(&mut self) {
        (self.table.device_drop)(self.handle);
    }
}

impl SdrDevice for PluginDevice {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn settings(&self) -> &DeviceSettings {
        &self.settings
    }

    fn apply(&mut self, settings: &DeviceSettings) -> Result<(), DeviceError> {
        let settings = serde_json::to_vec(settings)
            .map_err(|error| DeviceError::Io(format!("cannot encode the settings: {error}")))?;
        self.call(|table, handle| (table.apply)(handle, Str::of(&settings)))
    }

    fn rx_start(&mut self, sinks: Vec<RxSink>) -> Result<(), DeviceError> {
        let lent: Vec<Handle> = sinks.into_iter().map(sink::lend).collect();
        self.call(|table, handle| (table.rx_start)(handle, lent.as_ptr(), lent.len()))
    }

    fn rx_stop(&mut self) {
        if let Err(error) = self.call(|table, handle| (table.rx_stop)(handle)) {
            tracing::warn!(%error, "device plugin did not stop cleanly");
        }
    }

    fn duplex(&self) -> Duplex {
        let (table, handle) = (self.table, self.handle);
        json(table, |out| (table.duplex)(handle, out)).unwrap_or(self.capabilities.duplex)
    }

    fn in_flight_samples(&self) -> u64 {
        let mut samples = 0;
        match status(
            self.table,
            (self.table.in_flight_samples)(self.handle, &raw mut samples),
        ) {
            Ok(()) => samples,
            Err(error) => {
                tracing::warn!(%error, "device plugin in-flight count failed");
                0
            }
        }
    }

    fn tx_start_channels(&mut self, channels: &[u32]) -> Result<Box<dyn TxStream>, DeviceError> {
        let mut tx: Handle = std::ptr::null_mut();
        status(
            self.table,
            (self.table.tx_start)(self.handle, channels.as_ptr(), channels.len(), &raw mut tx),
        )?;
        let stream = Box::new(PluginTx {
            table: self.table,
            handle: tx,
        });
        self.refresh()?;
        Ok(stream)
    }

    fn agc_gains(&self) -> Result<Vec<AgcGain>, DeviceError> {
        let (table, handle) = (self.table, self.handle);
        json(table, |out| (table.agc_gains)(handle, out))
    }

    fn readouts(&self) -> Result<Vec<Readout>, DeviceError> {
        let (table, handle) = (self.table, self.handle);
        json(table, |out| (table.readouts)(handle, out))
    }

    fn set_noise_source(&mut self, on: bool) -> Result<(), DeviceError> {
        self.call(|table, handle| (table.set_noise_source)(handle, on))
    }

    fn sweep_start(&mut self, plan: &SweepPlan, sink: SweepSink) -> Result<(), DeviceError> {
        let bands: Vec<[f64; 2]> = plan
            .bands
            .iter()
            .map(|band| [band.start_hz, band.stop_hz])
            .collect();
        let plan = SweepPlanV1 {
            bands: bands.as_ptr(),
            band_count: bands.len(),
            sample_rate_hz: plan.sample_rate_hz,
        };
        let lent = sink::lend_sweep(sink);
        self.call(|table, handle| (table.sweep_start)(handle, &raw const plan, lent))
    }

    fn sweep_stop(&mut self) {
        if let Err(error) = self.call(|table, handle| (table.sweep_stop)(handle)) {
            tracing::warn!(%error, "device plugin did not stop its sweep cleanly");
        }
    }
}

struct PluginTx {
    table: &'static PluginV1,
    handle: Handle,
}

/// As [`PluginDevice`]: the stream is `Send` like the `TxStream` it wraps.
unsafe impl Send for PluginTx {}

impl TxStream for PluginTx {
    fn write_channels(
        &mut self,
        channels: &[&[Sample]],
        timeout: Duration,
        end_burst: bool,
    ) -> Result<usize, DeviceError> {
        if channels.len() > MAX_TX_CHANNELS {
            return Err(DeviceError::Unsupported(format!(
                "a plugin writes at most {MAX_TX_CHANNELS} channels at once, got {}",
                channels.len()
            )));
        }
        let mut given = [SamplesV1 {
            ptr: std::ptr::null(),
            len: 0,
        }; MAX_TX_CHANNELS];
        for (slot, channel) in given.iter_mut().zip(channels) {
            *slot = SamplesV1 {
                ptr: channel.as_ptr(),
                len: channel.len(),
            };
        }
        let mut written = 0;
        status(
            self.table,
            (self.table.tx_write)(
                self.handle,
                given.as_ptr(),
                channels.len(),
                u64::try_from(timeout.as_nanos()).unwrap_or(u64::MAX),
                end_burst,
                &raw mut written,
            ),
        )?;
        Ok(written)
    }

    fn stop(&mut self) -> Result<(), DeviceError> {
        status(self.table, (self.table.tx_stop)(self.handle))
    }
}

impl Drop for PluginTx {
    fn drop(&mut self) {
        (self.table.tx_drop)(self.handle);
    }
}

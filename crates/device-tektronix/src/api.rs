use std::{
    ffi::{CStr, c_char, c_int},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use sdrmm_device::{DeviceError, lock};

use crate::{
    ffi,
    model::{Chunk, Found, FrequencySource, Limits},
};

pub const OVERRIDE_VAR: &str = "SDRMM_TEKTRONIX_DIR";
pub const RETRY_AFTER: Duration = Duration::from_secs(5);

/// The calls this driver makes into the RSA API, so tests can stand in for the library. The API
/// drives one analyser per process, so nothing here names a device once it is connected.
pub trait Rsa: Send + Sync {
    fn version(&self) -> String;
    fn library_path(&self) -> &Path;
    fn devices(&self) -> Result<Vec<Found>, DeviceError>;
    fn connect(&self, found: &Found) -> Result<(), DeviceError>;
    fn disconnect(&self) -> Result<(), DeviceError>;
    fn limits(&self) -> Result<Limits, DeviceError>;
    fn center(&self) -> Result<f64, DeviceError>;
    fn set_center(&self, hz: f64) -> Result<(), DeviceError>;
    fn reference(&self) -> Result<f64, DeviceError>;
    fn set_reference(&self, dbm: f64) -> Result<(), DeviceError>;
    fn preamp(&self) -> Result<bool, DeviceError>;
    fn set_preamp(&self, on: bool) -> Result<(), DeviceError>;
    fn auto_attenuation(&self) -> Result<bool, DeviceError>;
    fn set_auto_attenuation(&self, on: bool) -> Result<(), DeviceError>;
    fn attenuation(&self) -> Result<f64, DeviceError>;
    fn set_attenuation(&self, db: f64) -> Result<(), DeviceError>;
    fn frequency_source(&self) -> Result<FrequencySource, DeviceError>;
    fn set_frequency_source(&self, source: FrequencySource) -> Result<(), DeviceError>;
    fn gnss_fitted(&self) -> Result<bool, DeviceError>;
    fn set_bandwidth(&self, hz: f64) -> Result<(), DeviceError>;
    fn rate(&self) -> Result<f64, DeviceError>;
    fn timestamp_rate(&self) -> Result<u64, DeviceError>;
    fn run(&self) -> Result<(), DeviceError>;
    fn stop(&self) -> Result<(), DeviceError>;
    fn stream_start(&self) -> Result<(), DeviceError>;
    fn stream_stop(&self) -> Result<(), DeviceError>;
    fn wait(&self, timeout: Duration) -> Result<bool, DeviceError>;
    fn read(&self, out: &mut [f32]) -> Result<Option<Chunk>, DeviceError>;
}

#[must_use]
pub fn classify(status: ffi::ReturnStatus, message: String) -> DeviceError {
    match status {
        ffi::ERROR_NOT_CONNECTED
        | ffi::ERROR_TRANSFER
        | ffi::ERROR_DISCONNECTED_FIRST..=ffi::ERROR_DISCONNECTED_LAST => {
            DeviceError::Disconnected(message)
        }
        ffi::ERROR_PARAMETER
        | ffi::ERROR_FREQUENCY_OUT_OF_RANGE
        | ffi::ERROR_IQ_STREAM_BANDWIDTH_OUT_OF_RANGE => DeviceError::Unsupported(message),
        _ => DeviceError::Io(message),
    }
}

struct Entries {
    error_string: ffi::ErrorStringFn,
    search: ffi::SearchFn,
    connect: ffi::ConnectFn,
    disconnect: ffi::PlainFn,
    run: ffi::PlainFn,
    stop: ffi::PlainFn,
    center_min: ffi::GetDoubleFn,
    center_max: ffi::GetDoubleFn,
    set_center: ffi::SetDoubleFn,
    get_center: ffi::GetDoubleFn,
    set_reference: ffi::SetDoubleFn,
    get_reference: ffi::GetDoubleFn,
    set_preamp: ffi::SetBoolFn,
    get_preamp: ffi::GetBoolFn,
    set_auto_attenuation: ffi::SetBoolFn,
    get_auto_attenuation: ffi::GetBoolFn,
    set_attenuation: ffi::SetDoubleFn,
    get_attenuation: ffi::GetDoubleFn,
    set_frequency_source: ffi::SetIntFn,
    get_frequency_source: ffi::GetIntFn,
    gnss_fitted: ffi::GetBoolFn,
    timestamp_rate: ffi::GetU64Fn,
    bandwidth_min: ffi::GetDoubleFn,
    bandwidth_max: ffi::GetDoubleFn,
    set_bandwidth: ffi::SetDoubleFn,
    acq_parameters: ffi::GetTwoDoublesFn,
    output: ffi::OutputFn,
    buffer_size: ffi::GetIntFn,
    stream_start: ffi::PlainFn,
    stream_stop: ffi::PlainFn,
    wait: ffi::WaitFn,
    iq_data: ffi::IqDataFn,
}

#[derive(Default)]
struct Session {
    connected: Option<Found>,
    block_pairs: usize,
}

pub struct LoadedApi {
    entries: Entries,
    session: Mutex<Session>,
    version: String,
    path: PathBuf,
    _library: libloading::Library,
}

macro_rules! entry {
    ($library:expr, $name:literal, $signature:ty) => {{
        let symbol: libloading::Symbol<'_, $signature> =
            unsafe { $library.get($name) }.map_err(|error| {
                let name = String::from_utf8_lossy($name);
                format!("{} is missing: {error}", name.trim_end_matches('\0'))
            })?;
        *symbol
    }};
}

impl LoadedApi {
    fn open(install: &Install) -> Result<Self, String> {
        let library = load_library(&install.library)?;
        let api_version: ffi::TextFn = entry!(library, b"DEVICE_GetAPIVersion\0", ffi::TextFn);
        let entries = Entries {
            error_string: entry!(library, b"DEVICE_GetErrorString\0", ffi::ErrorStringFn),
            search: entry!(library, b"DEVICE_Search\0", ffi::SearchFn),
            connect: entry!(library, b"DEVICE_Connect\0", ffi::ConnectFn),
            disconnect: entry!(library, b"DEVICE_Disconnect\0", ffi::PlainFn),
            run: entry!(library, b"DEVICE_Run\0", ffi::PlainFn),
            stop: entry!(library, b"DEVICE_Stop\0", ffi::PlainFn),
            center_min: entry!(library, b"CONFIG_GetMinCenterFreq\0", ffi::GetDoubleFn),
            center_max: entry!(library, b"CONFIG_GetMaxCenterFreq\0", ffi::GetDoubleFn),
            set_center: entry!(library, b"CONFIG_SetCenterFreq\0", ffi::SetDoubleFn),
            get_center: entry!(library, b"CONFIG_GetCenterFreq\0", ffi::GetDoubleFn),
            set_reference: entry!(library, b"CONFIG_SetReferenceLevel\0", ffi::SetDoubleFn),
            get_reference: entry!(library, b"CONFIG_GetReferenceLevel\0", ffi::GetDoubleFn),
            set_preamp: entry!(library, b"CONFIG_SetRFPreampEnable\0", ffi::SetBoolFn),
            get_preamp: entry!(library, b"CONFIG_GetRFPreampEnable\0", ffi::GetBoolFn),
            set_auto_attenuation: entry!(
                library,
                b"CONFIG_SetAutoAttenuationEnable\0",
                ffi::SetBoolFn
            ),
            get_auto_attenuation: entry!(
                library,
                b"CONFIG_GetAutoAttenuationEnable\0",
                ffi::GetBoolFn
            ),
            set_attenuation: entry!(library, b"CONFIG_SetRFAttenuator\0", ffi::SetDoubleFn),
            get_attenuation: entry!(library, b"CONFIG_GetRFAttenuator\0", ffi::GetDoubleFn),
            set_frequency_source: entry!(
                library,
                b"CONFIG_SetFrequencyReferenceSource\0",
                ffi::SetIntFn
            ),
            get_frequency_source: entry!(
                library,
                b"CONFIG_GetFrequencyReferenceSource\0",
                ffi::GetIntFn
            ),
            gnss_fitted: entry!(library, b"GNSS_GetHwInstalled\0", ffi::GetBoolFn),
            timestamp_rate: entry!(library, b"REFTIME_GetTimestampRate\0", ffi::GetU64Fn),
            bandwidth_min: entry!(library, b"IQSTREAM_GetMinAcqBandwidth\0", ffi::GetDoubleFn),
            bandwidth_max: entry!(library, b"IQSTREAM_GetMaxAcqBandwidth\0", ffi::GetDoubleFn),
            set_bandwidth: entry!(library, b"IQSTREAM_SetAcqBandwidth\0", ffi::SetDoubleFn),
            acq_parameters: entry!(
                library,
                b"IQSTREAM_GetAcqParameters\0",
                ffi::GetTwoDoublesFn
            ),
            output: entry!(library, b"IQSTREAM_SetOutputConfiguration\0", ffi::OutputFn),
            buffer_size: entry!(library, b"IQSTREAM_GetIQDataBufferSize\0", ffi::GetIntFn),
            stream_start: entry!(library, b"IQSTREAM_Start\0", ffi::PlainFn),
            stream_stop: entry!(library, b"IQSTREAM_Stop\0", ffi::PlainFn),
            wait: entry!(library, b"IQSTREAM_WaitForIQDataReady\0", ffi::WaitFn),
            iq_data: entry!(library, b"IQSTREAM_GetIQData\0", ffi::IqDataFn),
        };
        let mut version: [c_char; ffi::INFO_LEN] = [0; ffi::INFO_LEN];
        let status = unsafe { api_version(version.as_mut_ptr()) };
        if status != ffi::NO_ERROR {
            return Err(format!("the RSA API did not report its version ({status})"));
        }
        Ok(Self {
            entries,
            session: Mutex::new(Session::default()),
            version: ffi::text(&version),
            path: install.library.clone(),
            _library: library,
        })
    }

    fn describe(&self, status: ffi::ReturnStatus) -> String {
        let text = unsafe { (self.entries.error_string)(status) };
        if text.is_null() {
            return format!("status {status}");
        }
        let text = unsafe { CStr::from_ptr(text) }.to_string_lossy();
        format!("{} ({status})", text.trim())
    }

    fn check(&self, status: ffi::ReturnStatus, action: &str) -> Result<(), DeviceError> {
        if status == ffi::NO_ERROR {
            return Ok(());
        }
        Err(classify(
            status,
            format!("{action}: {}", self.describe(status)),
        ))
    }

    fn plain(&self, call: ffi::PlainFn, action: &str) -> Result<(), DeviceError> {
        let _session = lock(&self.session);
        self.check(unsafe { call() }, action)
    }

    fn set(&self, call: ffi::SetDoubleFn, value: f64, action: &str) -> Result<(), DeviceError> {
        let _session = lock(&self.session);
        self.check(unsafe { call(value) }, action)
    }

    fn get(&self, call: ffi::GetDoubleFn, action: &str) -> Result<f64, DeviceError> {
        let _session = lock(&self.session);
        let mut value = 0.0;
        self.check(unsafe { call(&raw mut value) }, action)?;
        Ok(value)
    }

    fn switch(&self, call: ffi::SetBoolFn, on: bool, action: &str) -> Result<(), DeviceError> {
        let _session = lock(&self.session);
        self.check(unsafe { call(on) }, action)
    }

    fn switched(&self, call: ffi::GetBoolFn, action: &str) -> Result<bool, DeviceError> {
        let _session = lock(&self.session);
        let mut on = false;
        self.check(unsafe { call(&raw mut on) }, action)?;
        Ok(on)
    }

    fn search(&self) -> Result<Vec<Found>, DeviceError> {
        let mut count: c_int = 0;
        let mut ids: [c_int; ffi::SEARCH_MAX_DEVICES] = [0; ffi::SEARCH_MAX_DEVICES];
        let mut serials: ffi::SerialBuffer = [[0; ffi::SEARCH_SERIAL_LEN]; ffi::SEARCH_MAX_DEVICES];
        let mut types: ffi::TypeBuffer = [[0; ffi::SEARCH_TYPE_LEN]; ffi::SEARCH_MAX_DEVICES];
        let status = unsafe {
            (self.entries.search)(
                &raw mut count,
                ids.as_mut_ptr(),
                serials.as_mut_ptr(),
                types.as_mut_ptr(),
            )
        };
        self.check(status, "searching for Tektronix analysers")?;
        let count = usize::try_from(count)
            .unwrap_or(0)
            .min(ffi::SEARCH_MAX_DEVICES);
        Ok((0..count)
            .map(|index| Found {
                id: ids[index],
                serial: ffi::text(&serials[index]),
                model: ffi::text(&types[index]),
            })
            .filter(|found| !found.serial.is_empty())
            .collect())
    }
}

impl Rsa for LoadedApi {
    fn version(&self) -> String {
        self.version.clone()
    }

    fn library_path(&self) -> &Path {
        &self.path
    }

    fn devices(&self) -> Result<Vec<Found>, DeviceError> {
        let session = lock(&self.session);
        match &session.connected {
            Some(found) => Ok(vec![found.clone()]),
            None => self.search(),
        }
    }

    fn connect(&self, found: &Found) -> Result<(), DeviceError> {
        let mut session = lock(&self.session);
        if let Some(open) = &session.connected {
            return Err(DeviceError::InUse(format!(
                "the RSA API drives one analyser per process and has {} open",
                open.serial
            )));
        }
        let status = unsafe { (self.entries.connect)(found.id) };
        self.check(status, "connecting to the analyser")?;
        session.connected = Some(found.clone());
        Ok(())
    }

    fn disconnect(&self) -> Result<(), DeviceError> {
        let mut session = lock(&self.session);
        session.connected = None;
        session.block_pairs = 0;
        self.check(
            unsafe { (self.entries.disconnect)() },
            "disconnecting the analyser",
        )
    }

    fn limits(&self) -> Result<Limits, DeviceError> {
        Ok(Limits {
            center_min: self.get(self.entries.center_min, "reading the lowest tuning")?,
            center_max: self.get(self.entries.center_max, "reading the highest tuning")?,
            bandwidth_min: self.get(self.entries.bandwidth_min, "reading the narrowest stream")?,
            bandwidth_max: self.get(self.entries.bandwidth_max, "reading the widest stream")?,
        })
    }

    fn center(&self) -> Result<f64, DeviceError> {
        self.get(self.entries.get_center, "reading the centre frequency")
    }

    fn set_center(&self, hz: f64) -> Result<(), DeviceError> {
        self.set(self.entries.set_center, hz, "setting the centre frequency")
    }

    fn reference(&self) -> Result<f64, DeviceError> {
        self.get(self.entries.get_reference, "reading the reference level")
    }

    fn set_reference(&self, dbm: f64) -> Result<(), DeviceError> {
        self.set(
            self.entries.set_reference,
            dbm,
            "setting the reference level",
        )
    }

    fn preamp(&self) -> Result<bool, DeviceError> {
        self.switched(self.entries.get_preamp, "reading the preamplifier")
    }

    fn set_preamp(&self, on: bool) -> Result<(), DeviceError> {
        self.switch(self.entries.set_preamp, on, "switching the preamplifier")
    }

    fn auto_attenuation(&self) -> Result<bool, DeviceError> {
        self.switched(
            self.entries.get_auto_attenuation,
            "reading the attenuator mode",
        )
    }

    fn set_auto_attenuation(&self, on: bool) -> Result<(), DeviceError> {
        self.switch(
            self.entries.set_auto_attenuation,
            on,
            "switching the attenuator mode",
        )
    }

    fn attenuation(&self) -> Result<f64, DeviceError> {
        self.get(self.entries.get_attenuation, "reading the attenuator")
    }

    fn set_attenuation(&self, db: f64) -> Result<(), DeviceError> {
        self.set(self.entries.set_attenuation, db, "setting the attenuator")
    }

    /// The API's fourth source is the internal oscillator with the operator's own correction.
    fn frequency_source(&self) -> Result<FrequencySource, DeviceError> {
        let _session = lock(&self.session);
        let mut source: c_int = ffi::SOURCE_INTERNAL;
        let status = unsafe { (self.entries.get_frequency_source)(&raw mut source) };
        self.check(status, "reading the frequency reference")?;
        Ok(match source {
            ffi::SOURCE_EXTERNAL => FrequencySource::External,
            ffi::SOURCE_GNSS => FrequencySource::Gnss,
            _ => FrequencySource::Internal,
        })
    }

    fn set_frequency_source(&self, source: FrequencySource) -> Result<(), DeviceError> {
        let _session = lock(&self.session);
        let source = match source {
            FrequencySource::Internal => ffi::SOURCE_INTERNAL,
            FrequencySource::External => ffi::SOURCE_EXTERNAL,
            FrequencySource::Gnss => ffi::SOURCE_GNSS,
        };
        self.check(
            unsafe { (self.entries.set_frequency_source)(source) },
            "switching the frequency reference",
        )
    }

    fn gnss_fitted(&self) -> Result<bool, DeviceError> {
        self.switched(self.entries.gnss_fitted, "reading whether GNSS is fitted")
    }

    fn set_bandwidth(&self, hz: f64) -> Result<(), DeviceError> {
        self.set(self.entries.set_bandwidth, hz, "setting the IQ bandwidth")
    }

    fn rate(&self) -> Result<f64, DeviceError> {
        let _session = lock(&self.session);
        let mut bandwidth = 0.0;
        let mut rate = 0.0;
        let status = unsafe { (self.entries.acq_parameters)(&raw mut bandwidth, &raw mut rate) };
        self.check(status, "reading the IQ sample rate")?;
        Ok(rate)
    }

    fn timestamp_rate(&self) -> Result<u64, DeviceError> {
        let _session = lock(&self.session);
        let mut rate = 0;
        let status = unsafe { (self.entries.timestamp_rate)(&raw mut rate) };
        self.check(status, "reading the timestamp rate")?;
        Ok(rate)
    }

    fn run(&self) -> Result<(), DeviceError> {
        self.plain(self.entries.run, "starting the acquisition")
    }

    fn stop(&self) -> Result<(), DeviceError> {
        self.plain(self.entries.stop, "stopping the acquisition")
    }

    fn stream_start(&self) -> Result<(), DeviceError> {
        let mut session = lock(&self.session);
        let status = unsafe { (self.entries.output)(ffi::OUTPUT_CLIENT, ffi::OUTPUT_SINGLE) };
        self.check(status, "routing the IQ stream")?;
        let mut pairs: c_int = 0;
        let status = unsafe { (self.entries.buffer_size)(&raw mut pairs) };
        self.check(status, "reading the IQ block size")?;
        session.block_pairs = usize::try_from(pairs).unwrap_or(0);
        self.check(
            unsafe { (self.entries.stream_start)() },
            "starting the IQ stream",
        )
    }

    fn stream_stop(&self) -> Result<(), DeviceError> {
        self.plain(self.entries.stream_stop, "stopping the IQ stream")
    }

    /// Not serialised with the other calls: it only blocks on the stream's next block, and a
    /// retune has to get through while the capture thread waits here.
    fn wait(&self, timeout: Duration) -> Result<bool, DeviceError> {
        let millis = c_int::try_from(timeout.as_millis()).unwrap_or(c_int::MAX);
        let mut ready = false;
        let status = unsafe { (self.entries.wait)(millis, &raw mut ready) };
        if status == ffi::ERROR_IQ_STREAMING_NOT_ENABLED {
            std::thread::sleep(timeout);
            return Ok(false);
        }
        self.check(status, "waiting for IQ data")?;
        Ok(ready)
    }

    fn read(&self, out: &mut [f32]) -> Result<Option<Chunk>, DeviceError> {
        let session = lock(&self.session);
        if session.block_pairs == 0 {
            return Ok(None);
        }
        if out.len() < session.block_pairs * 2 {
            return Err(DeviceError::Io(format!(
                "the RSA API hands out blocks of {} samples, more than the {} there is room for",
                session.block_pairs,
                out.len() / 2
            )));
        }
        let mut pairs: c_int = 0;
        let mut info = ffi::IqInfo::default();
        let status = unsafe {
            (self.entries.iq_data)(out.as_mut_ptr().cast(), &raw mut pairs, &raw mut info)
        };
        if matches!(
            status,
            ffi::ERROR_IQ_STREAMING_NOT_ENABLED | ffi::ERROR_DATA_NOT_READY
        ) {
            return Ok(None);
        }
        self.check(status, "reading IQ data")?;
        let pairs = usize::try_from(pairs).unwrap_or(0);
        Ok((pairs > 0).then_some(Chunk {
            pairs,
            timestamp: info.timestamp,
            status: info.acq_status,
        }))
    }
}

impl Drop for LoadedApi {
    fn drop(&mut self) {
        if lock(&self.session).connected.is_some() {
            unsafe { (self.entries.disconnect)() };
        }
    }
}

/// Where an RSA API library may be. One named without a folder is left to the system's own
/// library search.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Install {
    pub library: PathBuf,
    pub searched: bool,
}

impl Install {
    fn at(library: PathBuf) -> Self {
        Self {
            library,
            searched: false,
        }
    }

    fn present(&self) -> bool {
        self.searched || self.library.exists()
    }
}

#[cfg(windows)]
const LIBRARY_PATHS: &[&[&str]] = &[&["RSA_API.dll"], &["lib", "x64", "RSA_API.dll"]];
#[cfg(not(windows))]
const LIBRARY_PATHS: &[&[&str]] = &[&["libRSA_API.so"]];

fn libraries_in(dir: &Path) -> impl Iterator<Item = Install> + '_ {
    LIBRARY_PATHS.iter().map(move |parts| {
        Install::at(
            parts
                .iter()
                .fold(dir.to_path_buf(), |path, part| path.join(part)),
        )
    })
}

#[cfg(windows)]
fn default_installs() -> Vec<Install> {
    libraries_in(Path::new("C:\\Tektronix\\RSA_API")).collect()
}

#[cfg(target_os = "linux")]
fn default_installs() -> Vec<Install> {
    vec![Install {
        library: PathBuf::from("libRSA_API.so"),
        searched: true,
    }]
}

#[cfg(not(any(windows, target_os = "linux")))]
fn default_installs() -> Vec<Install> {
    Vec::new()
}

#[must_use]
pub fn candidates() -> Vec<Install> {
    let overridden = std::env::var_os(OVERRIDE_VAR).map(PathBuf::from);
    overridden
        .iter()
        .flat_map(|dir| libraries_in(dir))
        .chain(default_installs())
        .collect()
}

/// Whether this platform has an RSA API to look for at all; Tektronix ships none for macOS.
#[must_use]
pub fn platform_supported() -> bool {
    cfg!(any(windows, target_os = "linux")) || std::env::var_os(OVERRIDE_VAR).is_some()
}

#[cfg(windows)]
fn load_library(path: &Path) -> Result<libloading::Library, String> {
    use libloading::os::windows::{
        LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, Library,
    };
    unsafe {
        Library::load_with_flags(
            path,
            LOAD_LIBRARY_SEARCH_DEFAULT_DIRS | LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
        )
    }
    .map(libloading::Library::from)
    .map_err(|error| format!("{} could not be loaded: {error}", path.display()))
}

#[cfg(not(windows))]
fn load_library(path: &Path) -> Result<libloading::Library, String> {
    unsafe { libloading::Library::new(path) }
        .map_err(|error| format!("{} could not be loaded: {error}", path.display()))
}

fn load() -> Result<Arc<LoadedApi>, String> {
    let mut failures = Vec::new();
    for install in candidates() {
        if !install.present() {
            continue;
        }
        match LoadedApi::open(&install) {
            Ok(api) => {
                tracing::info!(path = %api.path.display(), "loaded the Tektronix RSA API");
                return Ok(Arc::new(api));
            }
            Err(error) => failures.push(error),
        }
    }
    Err(if failures.is_empty() {
        "the Tektronix RSA API is not installed".to_string()
    } else {
        failures.join("; ")
    })
}

#[derive(Default)]
struct Loader {
    api: Option<Arc<LoadedApi>>,
    failure: Option<(String, Instant)>,
}

impl Loader {
    fn resolve_with<T: Clone>(
        slot: &mut Option<T>,
        failure: &mut Option<(String, Instant)>,
        now: Instant,
        load: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        if let Some(api) = slot {
            return Ok(api.clone());
        }
        if let Some((error, at)) = failure
            && now.duration_since(*at) < RETRY_AFTER
        {
            return Err(error.clone());
        }
        match load() {
            Ok(api) => {
                *slot = Some(api.clone());
                *failure = None;
                Ok(api)
            }
            Err(error) => {
                let repeated = failure
                    .as_ref()
                    .is_some_and(|(previous, _)| *previous == error);
                if !repeated {
                    tracing::warn!("the Tektronix RSA API is unavailable: {error}");
                }
                *failure = Some((error.clone(), now));
                Err(error)
            }
        }
    }
}

static LOADER: Mutex<Loader> = Mutex::new(Loader {
    api: None,
    failure: None,
});

pub fn shared() -> Result<Arc<dyn Rsa>, String> {
    if !platform_supported() {
        return Err("Tektronix ships no RSA API for this platform".to_string());
    }
    let mut loader = LOADER.lock().unwrap_or_else(PoisonError::into_inner);
    let Loader { api, failure } = &mut *loader;
    Loader::resolve_with(api, failure, Instant::now(), load).map(|api| api as Arc<dyn Rsa>)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pulled_cable_reads_as_disconnected() {
        for status in [ffi::ERROR_NOT_CONNECTED, ffi::ERROR_TRANSFER, 3101, 3105] {
            assert!(
                matches!(
                    classify(status, "gone".to_string()),
                    DeviceError::Disconnected(_)
                ),
                "{status}"
            );
        }
    }

    #[test]
    fn a_rejected_value_reads_as_unsupported() {
        for status in [
            ffi::ERROR_PARAMETER,
            ffi::ERROR_FREQUENCY_OUT_OF_RANGE,
            ffi::ERROR_IQ_STREAM_BANDWIDTH_OUT_OF_RANGE,
        ] {
            assert!(
                matches!(
                    classify(status, "refused".to_string()),
                    DeviceError::Unsupported(_)
                ),
                "{status}"
            );
        }
    }

    #[test]
    fn anything_else_is_an_io_error() {
        assert!(matches!(
            classify(3001, "timeout".to_string()),
            DeviceError::Io(_)
        ));
    }

    #[test]
    fn an_install_names_the_library_inside_it() {
        let dir = Path::new("/somewhere");
        let installs: Vec<Install> = libraries_in(dir).collect();
        assert!(!installs.is_empty());
        assert!(
            installs
                .iter()
                .all(|install| install.library.starts_with(dir) && !install.searched)
        );
    }

    #[test]
    fn a_failed_load_is_held_briefly_and_then_tried_again() {
        let attempts = std::cell::Cell::new(0);
        let mut slot: Option<u8> = None;
        let mut failure = None;
        let start = Instant::now();
        let mut attempt = |now| {
            let result = Loader::resolve_with(&mut slot, &mut failure, now, || {
                attempts.set(attempts.get() + 1);
                Err::<u8, _>("not installed".to_string())
            });
            assert!(result.is_err());
        };
        attempt(start);
        attempt(start + RETRY_AFTER / 2);
        assert_eq!(attempts.get(), 1);
        attempt(start + RETRY_AFTER);
        assert_eq!(attempts.get(), 2);
    }
}

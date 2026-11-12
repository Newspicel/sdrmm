use std::{
    ffi::c_void,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use sdrmm_device::DeviceError;

use crate::{
    ffi,
    model::{Found, Item, ItemKind, Kind, Mode, Outgoing, PacketView},
};

pub const OVERRIDE_VAR: &str = "SDRMM_AARONIA_DIR";
pub const RETRY_AFTER: Duration = Duration::from_secs(5);
const RESCAN_ATTEMPTS: usize = 4;

/// An opened device, by the address the API gave it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DevHandle(pub usize);

impl DevHandle {
    fn raw(self) -> ffi::Device {
        ffi::Device {
            d: self.0 as *mut c_void,
        }
    }
}

/// The calls this driver makes into the RTSA API, so tests can stand in for the library.
pub trait Rtsa: Send + Sync {
    fn version(&self) -> u32;
    fn library_path(&self) -> &Path;
    fn open_count(&self) -> usize;
    fn devices(&self, rescan: Option<Duration>) -> Result<Vec<Found>, DeviceError>;
    fn open(&self, kind: Kind, mode: Mode, serial: &str) -> Result<DevHandle, DeviceError>;
    fn close(&self, dev: DevHandle) -> Result<(), DeviceError>;
    fn connect(&self, dev: DevHandle) -> Result<(), DeviceError>;
    fn disconnect(&self, dev: DevHandle) -> Result<(), DeviceError>;
    fn start(&self, dev: DevHandle) -> Result<(), DeviceError>;
    fn stop(&self, dev: DevHandle) -> Result<(), DeviceError>;
    fn running(&self, dev: DevHandle) -> bool;
    fn item(&self, dev: DevHandle, path: &str) -> Result<Option<Item>, DeviceError>;
    fn get_float(&self, dev: DevHandle, path: &str) -> Result<f64, DeviceError>;
    fn set_float(&self, dev: DevHandle, path: &str, value: f64) -> Result<(), DeviceError>;
    fn get_string(&self, dev: DevHandle, path: &str) -> Result<String, DeviceError>;
    fn set_string(&self, dev: DevHandle, path: &str, value: &str) -> Result<(), DeviceError>;
    fn health(&self, dev: DevHandle, path: &str) -> Result<Option<f64>, DeviceError>;
    fn packet(&self, dev: DevHandle) -> Result<Option<PacketView>, DeviceError>;
    fn consume(&self, dev: DevHandle) -> Result<(), DeviceError>;
    fn send(&self, dev: DevHandle, packet: &Outgoing<'_>) -> Result<(), DeviceError>;
    fn stream_time(&self, dev: DevHandle) -> Result<f64, DeviceError>;
}

#[must_use]
pub fn meaning(code: ffi::ResultCode) -> &'static str {
    match code {
        ffi::ERROR_NOT_FOUND => "not found",
        ffi::ERROR_BUSY => "the device is in use",
        0x8000_0001 => "the library is not initialised",
        0x8000_0004 => "the device is not open",
        ffi::ERROR_NOT_CONNECTED => "the device is not connected",
        ffi::ERROR_INVALID_CONFIG => "invalid configuration",
        0x8000_0007 => "buffer too small",
        0x8000_0008 => "invalid channel",
        ffi::ERROR_INVALID_PARAMETER => "invalid parameter",
        0x8000_000a => "invalid size",
        ffi::ERROR_MISSING_PATHS_FILE => {
            "paths.xml is missing: reinstall the RTSA-Suite PRO or point SDRMM_AARONIA_DIR at it"
        }
        ffi::ERROR_VALUE_INVALID => "invalid value",
        ffi::ERROR_VALUE_MALFORMED => "malformed value",
        _ => "the RTSA API refused the request",
    }
}

/// Every code without the error bit is a success; states and warnings included.
pub fn check(code: ffi::ResultCode, action: &str) -> Result<(), DeviceError> {
    if code & ffi::ERROR == 0 {
        return Ok(());
    }
    let message = format!("{action}: {} (0x{code:08x})", meaning(code));
    Err(match code {
        ffi::ERROR_BUSY => DeviceError::InUse(message),
        ffi::ERROR_NOT_FOUND => DeviceError::NotFound(message),
        ffi::ERROR_NOT_CONNECTED => DeviceError::Disconnected(message),
        ffi::ERROR_INVALID_CONFIG
        | ffi::ERROR_INVALID_PARAMETER
        | ffi::ERROR_VALUE_INVALID
        | ffi::ERROR_VALUE_MALFORMED => DeviceError::Unsupported(message),
        _ => DeviceError::Io(message),
    })
}

struct Entries {
    shutdown: ffi::ShutdownFn,
    close: ffi::CloseFn,
    rescan: ffi::RescanFn,
    enum_device: ffi::EnumDeviceFn,
    open_device: ffi::OpenDeviceFn,
    close_device: ffi::CloseDeviceFn,
    connect: ffi::DeviceFn,
    disconnect: ffi::DeviceFn,
    start: ffi::DeviceFn,
    stop: ffi::DeviceFn,
    state: ffi::DeviceFn,
    get_packet: ffi::GetPacketFn,
    consume: ffi::ConsumeFn,
    stream_time: ffi::StreamTimeFn,
    send_packet: ffi::SendPacketFn,
    config_root: ffi::ConfigRootFn,
    config_health: ffi::ConfigRootFn,
    config_find: ffi::ConfigFindFn,
    config_info: ffi::ConfigInfoFn,
    set_float: ffi::SetFloatFn,
    get_float: ffi::GetFloatFn,
    set_string: ffi::SetStringFn,
    get_string: ffi::GetStringFn,
}

pub struct LoadedApi {
    entries: Entries,
    handle: Mutex<ffi::Handle>,
    opened: AtomicUsize,
    version: u32,
    path: PathBuf,
    _library: libloading::Library,
}

unsafe impl Send for LoadedApi {}
unsafe impl Sync for LoadedApi {}

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
        let library = load_library(install)?;
        let init: ffi::InitWithPathFn =
            entry!(library, b"AARTSAAPI_Init_With_Path\0", ffi::InitWithPathFn);
        let version: ffi::VersionFn = entry!(library, b"AARTSAAPI_Version\0", ffi::VersionFn);
        let open: ffi::OpenFn = entry!(library, b"AARTSAAPI_Open\0", ffi::OpenFn);
        let entries = Entries {
            shutdown: entry!(library, b"AARTSAAPI_Shutdown\0", ffi::ShutdownFn),
            close: entry!(library, b"AARTSAAPI_Close\0", ffi::CloseFn),
            rescan: entry!(library, b"AARTSAAPI_RescanDevices\0", ffi::RescanFn),
            enum_device: entry!(library, b"AARTSAAPI_EnumDevice\0", ffi::EnumDeviceFn),
            open_device: entry!(library, b"AARTSAAPI_OpenDevice\0", ffi::OpenDeviceFn),
            close_device: entry!(library, b"AARTSAAPI_CloseDevice\0", ffi::CloseDeviceFn),
            connect: entry!(library, b"AARTSAAPI_ConnectDevice\0", ffi::DeviceFn),
            disconnect: entry!(library, b"AARTSAAPI_DisconnectDevice\0", ffi::DeviceFn),
            start: entry!(library, b"AARTSAAPI_StartDevice\0", ffi::DeviceFn),
            stop: entry!(library, b"AARTSAAPI_StopDevice\0", ffi::DeviceFn),
            state: entry!(library, b"AARTSAAPI_GetDeviceState\0", ffi::DeviceFn),
            get_packet: entry!(library, b"AARTSAAPI_GetPacket\0", ffi::GetPacketFn),
            consume: entry!(library, b"AARTSAAPI_ConsumePackets\0", ffi::ConsumeFn),
            stream_time: entry!(
                library,
                b"AARTSAAPI_GetMasterStreamTime\0",
                ffi::StreamTimeFn
            ),
            send_packet: entry!(library, b"AARTSAAPI_SendPacket\0", ffi::SendPacketFn),
            config_root: entry!(library, b"AARTSAAPI_ConfigRoot\0", ffi::ConfigRootFn),
            config_health: entry!(library, b"AARTSAAPI_ConfigHealth\0", ffi::ConfigRootFn),
            config_find: entry!(library, b"AARTSAAPI_ConfigFind\0", ffi::ConfigFindFn),
            config_info: entry!(library, b"AARTSAAPI_ConfigGetInfo\0", ffi::ConfigInfoFn),
            set_float: entry!(library, b"AARTSAAPI_ConfigSetFloat\0", ffi::SetFloatFn),
            get_float: entry!(library, b"AARTSAAPI_ConfigGetFloat\0", ffi::GetFloatFn),
            set_string: entry!(library, b"AARTSAAPI_ConfigSetString\0", ffi::SetStringFn),
            get_string: entry!(library, b"AARTSAAPI_ConfigGetString\0", ffi::GetStringFn),
        };
        let dir = ffi::wide(&install.dir.to_string_lossy());
        let code = unsafe { init(ffi::MEMORY_MEDIUM, dir.as_ptr()) };
        check(code, "initialising the RTSA API").map_err(|error| error.to_string())?;
        let mut handle = ffi::Handle::default();
        if let Err(error) = check(unsafe { open(&raw mut handle) }, "opening the RTSA API") {
            unsafe { (entries.shutdown)() };
            return Err(error.to_string());
        }
        Ok(Self {
            entries,
            handle: Mutex::new(handle),
            opened: AtomicUsize::new(0),
            version: unsafe { version() },
            path: install.library.clone(),
            _library: library,
        })
    }

    fn with_handle<T>(&self, body: impl FnOnce(*mut ffi::Handle) -> T) -> T {
        let mut handle = self.handle.lock().unwrap_or_else(PoisonError::into_inner);
        body(&raw mut *handle)
    }

    fn device_call(
        &self,
        call: ffi::DeviceFn,
        dev: DevHandle,
        action: &str,
    ) -> Result<(), DeviceError> {
        let mut raw = dev.raw();
        check(unsafe { call(&raw mut raw) }, action)
    }

    fn find(&self, dev: DevHandle, health: bool, path: &str) -> Option<ffi::Config> {
        let mut raw = dev.raw();
        let mut root = ffi::Config::default();
        let tree = if health {
            self.entries.config_health
        } else {
            self.entries.config_root
        };
        if check(
            unsafe { tree(&raw mut raw, &raw mut root) },
            "reading the config tree",
        )
        .is_err()
        {
            return None;
        }
        let name = ffi::wide(path);
        let mut found = ffi::Config::default();
        let code = unsafe {
            (self.entries.config_find)(&raw mut raw, &raw mut root, &raw mut found, name.as_ptr())
        };
        (check(code, path).is_ok() && !found.d.is_null()).then_some(found)
    }

    fn require(&self, dev: DevHandle, path: &str) -> Result<ffi::Config, DeviceError> {
        self.find(dev, false, path)
            .ok_or_else(|| DeviceError::Unsupported(format!("this SPECTRAN has no {path}")))
    }

    fn enumerate(&self, handle: *mut ffi::Handle, kind: Kind) -> Vec<Found> {
        let type_name = ffi::wide(kind.type_name());
        let mut found = Vec::new();
        for index in 0.. {
            let mut info = ffi::DeviceInfo::default();
            let code = unsafe {
                (self.entries.enum_device)(handle, type_name.as_ptr(), index, &raw mut info)
            };
            if code != ffi::OK {
                break;
            }
            found.push(Found {
                kind,
                serial: ffi::narrow(&info.serial_number).trim().to_string(),
                ready: info.ready != 0,
                active: info.active != 0,
            });
        }
        found
    }

    fn rescan(&self, handle: *mut ffi::Handle, timeout: Duration) -> Result<(), DeviceError> {
        let millis = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
        for _ in 0..RESCAN_ATTEMPTS {
            let code = unsafe { (self.entries.rescan)(handle, millis) };
            if code != ffi::RETRY {
                return check(code, "rescanning SPECTRAN devices");
            }
        }
        Ok(())
    }
}

impl Rtsa for LoadedApi {
    fn version(&self) -> u32 {
        self.version
    }

    fn library_path(&self) -> &Path {
        &self.path
    }

    fn open_count(&self) -> usize {
        self.opened.load(Ordering::Acquire)
    }

    fn devices(&self, rescan: Option<Duration>) -> Result<Vec<Found>, DeviceError> {
        self.with_handle(|handle| {
            if let Some(timeout) = rescan {
                self.rescan(handle, timeout)?;
            }
            Ok(Kind::ALL
                .iter()
                .flat_map(|&kind| self.enumerate(handle, kind))
                .filter(|found| !found.serial.is_empty())
                .collect())
        })
    }

    fn open(&self, kind: Kind, mode: Mode, serial: &str) -> Result<DevHandle, DeviceError> {
        let type_name = ffi::wide(&mode.device_type(kind));
        let serial = ffi::wide(serial);
        let mut raw = ffi::Device::default();
        let code = self.with_handle(|handle| unsafe {
            (self.entries.open_device)(handle, &raw mut raw, type_name.as_ptr(), serial.as_ptr())
        });
        check(code, "opening the SPECTRAN")?;
        self.opened.fetch_add(1, Ordering::AcqRel);
        Ok(DevHandle(raw.d as usize))
    }

    fn close(&self, dev: DevHandle) -> Result<(), DeviceError> {
        let mut raw = dev.raw();
        let code =
            self.with_handle(|handle| unsafe { (self.entries.close_device)(handle, &raw mut raw) });
        self.opened.fetch_sub(1, Ordering::AcqRel);
        check(code, "closing the SPECTRAN")
    }

    fn connect(&self, dev: DevHandle) -> Result<(), DeviceError> {
        self.device_call(self.entries.connect, dev, "connecting to the SPECTRAN")
    }

    fn disconnect(&self, dev: DevHandle) -> Result<(), DeviceError> {
        self.device_call(self.entries.disconnect, dev, "disconnecting the SPECTRAN")
    }

    fn start(&self, dev: DevHandle) -> Result<(), DeviceError> {
        self.device_call(self.entries.start, dev, "starting the SPECTRAN")
    }

    fn stop(&self, dev: DevHandle) -> Result<(), DeviceError> {
        self.device_call(self.entries.stop, dev, "stopping the SPECTRAN")
    }

    fn running(&self, dev: DevHandle) -> bool {
        let mut raw = dev.raw();
        unsafe { (self.entries.state)(&raw mut raw) == ffi::RUNNING }
    }

    fn item(&self, dev: DevHandle, path: &str) -> Result<Option<Item>, DeviceError> {
        let Some(mut config) = self.find(dev, false, path) else {
            return Ok(None);
        };
        let mut raw = dev.raw();
        let mut info = ffi::ConfigInfo::default();
        check(
            unsafe { (self.entries.config_info)(&raw mut raw, &raw mut config, &raw mut info) },
            path,
        )?;
        Ok(Some(item_from(&info)))
    }

    fn get_float(&self, dev: DevHandle, path: &str) -> Result<f64, DeviceError> {
        let mut config = self.require(dev, path)?;
        let mut raw = dev.raw();
        let mut value = 0.0;
        check(
            unsafe { (self.entries.get_float)(&raw mut raw, &raw mut config, &raw mut value) },
            path,
        )?;
        Ok(value)
    }

    fn set_float(&self, dev: DevHandle, path: &str, value: f64) -> Result<(), DeviceError> {
        let mut config = self.require(dev, path)?;
        let mut raw = dev.raw();
        check(
            unsafe { (self.entries.set_float)(&raw mut raw, &raw mut config, value) },
            path,
        )
    }

    fn get_string(&self, dev: DevHandle, path: &str) -> Result<String, DeviceError> {
        let mut config = self.require(dev, path)?;
        let mut raw = dev.raw();
        let mut buffer: Vec<ffi::WChar> = vec![0; ffi::STRING_LEN];
        let mut size = ffi::STRING_LEN as i64;
        check(
            unsafe {
                (self.entries.get_string)(
                    &raw mut raw,
                    &raw mut config,
                    buffer.as_mut_ptr(),
                    &raw mut size,
                )
            },
            path,
        )?;
        Ok(ffi::narrow(&buffer))
    }

    fn set_string(&self, dev: DevHandle, path: &str, value: &str) -> Result<(), DeviceError> {
        let mut config = self.require(dev, path)?;
        let mut raw = dev.raw();
        let value = ffi::wide(value);
        check(
            unsafe { (self.entries.set_string)(&raw mut raw, &raw mut config, value.as_ptr()) },
            path,
        )
    }

    fn health(&self, dev: DevHandle, path: &str) -> Result<Option<f64>, DeviceError> {
        let Some(mut config) = self.find(dev, true, path) else {
            return Ok(None);
        };
        let mut raw = dev.raw();
        let mut value = 0.0;
        check(
            unsafe { (self.entries.get_float)(&raw mut raw, &raw mut config, &raw mut value) },
            path,
        )?;
        Ok(Some(value))
    }

    fn packet(&self, dev: DevHandle) -> Result<Option<PacketView>, DeviceError> {
        let mut raw = dev.raw();
        let mut packet = ffi::Packet::default();
        let code = unsafe { (self.entries.get_packet)(&raw mut raw, 0, 0, &raw mut packet) };
        if code == ffi::EMPTY {
            return Ok(None);
        }
        check(code, "reading a SPECTRAN packet")?;
        let full = packet.time_base > 0.0;
        Ok(Some(PacketView {
            flags: packet.flags,
            start_time: if full {
                packet.time_base + packet.start_time_offset
            } else {
                packet.start_time
            },
            end_time: if full {
                packet.time_base + packet.end_time_offset
            } else {
                packet.end_time
            },
            step_hz: packet.step_frequency,
            num: packet.num,
            size: packet.size,
            stride: packet.stride,
            data: packet.fp32,
        }))
    }

    fn consume(&self, dev: DevHandle) -> Result<(), DeviceError> {
        let mut raw = dev.raw();
        check(
            unsafe { (self.entries.consume)(&raw mut raw, 0, 1) },
            "releasing a SPECTRAN packet",
        )
    }

    fn send(&self, dev: DevHandle, outgoing: &Outgoing<'_>) -> Result<(), DeviceError> {
        let mut raw = dev.raw();
        let packet = ffi::Packet {
            flags: outgoing.flags,
            start_time: outgoing.start_time,
            end_time: outgoing.end_time,
            start_frequency: outgoing.start_hz,
            step_frequency: outgoing.step_hz,
            span_frequency: outgoing.step_hz,
            num: (outgoing.iq.len() / 2) as i64,
            total: (outgoing.iq.len() / 2) as i64,
            size: 2,
            stride: 2,
            fp32: outgoing.iq.as_ptr().cast_mut(),
            ..ffi::Packet::default()
        };
        check(
            unsafe { (self.entries.send_packet)(&raw mut raw, 0, &raw const packet) },
            "sending a SPECTRAN packet",
        )
    }

    fn stream_time(&self, dev: DevHandle) -> Result<f64, DeviceError> {
        let mut raw = dev.raw();
        let mut time = 0.0;
        check(
            unsafe { (self.entries.stream_time)(&raw mut raw, &raw mut time) },
            "reading the SPECTRAN stream time",
        )?;
        Ok(time)
    }
}

impl Drop for LoadedApi {
    fn drop(&mut self) {
        self.with_handle(|handle| unsafe { (self.entries.close)(handle) });
        unsafe { (self.entries.shutdown)() };
    }
}

fn item_from(info: &ffi::ConfigInfo) -> Item {
    let options = ffi::narrow(&info.options);
    Item {
        kind: match info.kind {
            ffi::CONFIG_TYPE_NUMBER => ItemKind::Number,
            ffi::CONFIG_TYPE_BOOL => ItemKind::Bool,
            ffi::CONFIG_TYPE_ENUM => ItemKind::Enum,
            ffi::CONFIG_TYPE_STRING => ItemKind::String,
            _ => ItemKind::Other,
        },
        min: info.min_value,
        max: info.max_value,
        step: info.step_value,
        options: split_options(&options),
        disabled: info.disabled_options,
    }
}

#[must_use]
pub fn split_options(options: &str) -> Vec<String> {
    options
        .split(';')
        .map(str::trim)
        .filter(|option| !option.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Where an RTSA-Suite PRO is installed, and the API library inside it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Install {
    pub dir: PathBuf,
    pub library: PathBuf,
}

#[cfg(windows)]
const LIBRARY_PATHS: &[&[&str]] = &[&["sdk", "AaroniaRTSAAPI.dll"], &["AaroniaRTSAAPI.dll"]];
#[cfg(not(windows))]
const LIBRARY_PATHS: &[&[&str]] = &[&["libAaroniaRTSAAPI.so"]];

fn libraries_in(dir: &Path) -> impl Iterator<Item = Install> + '_ {
    LIBRARY_PATHS.iter().map(move |parts| Install {
        dir: dir.to_path_buf(),
        library: parts
            .iter()
            .fold(dir.to_path_buf(), |path, part| path.join(part)),
    })
}

fn install_dirs() -> Vec<PathBuf> {
    let overridden = std::env::var_os(OVERRIDE_VAR).map(PathBuf::from);
    overridden
        .into_iter()
        .chain(default_install_dir())
        .collect()
}

#[cfg(windows)]
fn default_install_dir() -> Option<PathBuf> {
    let program_files = std::env::var_os("ProgramFiles")
        .map_or_else(|| PathBuf::from("C:\\Program Files"), PathBuf::from);
    Some(
        program_files
            .join("Aaronia AG")
            .join("Aaronia RTSA-Suite PRO"),
    )
}

#[cfg(target_os = "linux")]
fn default_install_dir() -> Option<PathBuf> {
    Some(PathBuf::from(
        "/opt/aaronia-rtsa-suite/Aaronia-RTSA-Suite-PRO",
    ))
}

#[cfg(not(any(windows, target_os = "linux")))]
fn default_install_dir() -> Option<PathBuf> {
    None
}

#[must_use]
pub fn candidates() -> Vec<Install> {
    install_dirs()
        .iter()
        .flat_map(|dir| libraries_in(dir).collect::<Vec<_>>())
        .collect()
}

/// Whether this platform has an RTSA-Suite to look for at all; Aaronia ships none for macOS.
#[must_use]
pub fn platform_supported() -> bool {
    cfg!(any(windows, target_os = "linux")) || std::env::var_os(OVERRIDE_VAR).is_some()
}

#[cfg(windows)]
fn load_library(install: &Install) -> Result<libloading::Library, String> {
    use libloading::os::windows::{
        LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
        LOAD_LIBRARY_SEARCH_USER_DIRS, Library,
    };
    let dir = ffi::wide(&install.dir.to_string_lossy());
    let cookie =
        unsafe { windows_sys::Win32::System::LibraryLoader::AddDllDirectory(dir.as_ptr()) };
    let loaded = unsafe {
        Library::load_with_flags(
            &install.library,
            LOAD_LIBRARY_SEARCH_DEFAULT_DIRS
                | LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR
                | LOAD_LIBRARY_SEARCH_USER_DIRS,
        )
    };
    if !cookie.is_null() {
        unsafe { windows_sys::Win32::System::LibraryLoader::RemoveDllDirectory(cookie) };
    }
    loaded
        .map(libloading::Library::from)
        .map_err(|error| format!("{} could not be loaded: {error}", install.library.display()))
}

#[cfg(not(windows))]
fn load_library(install: &Install) -> Result<libloading::Library, String> {
    unsafe { libloading::Library::new(&install.library) }
        .map_err(|error| format!("{} could not be loaded: {error}", install.library.display()))
}

fn load() -> Result<Arc<LoadedApi>, String> {
    let mut failures = Vec::new();
    for install in candidates() {
        if !install.library.exists() {
            continue;
        }
        match LoadedApi::open(&install) {
            Ok(api) => {
                tracing::info!(path = %api.path.display(), "loaded the Aaronia RTSA API");
                return Ok(Arc::new(api));
            }
            Err(error) => failures.push(error),
        }
    }
    Err(if failures.is_empty() {
        "the Aaronia RTSA-Suite PRO is not installed".to_string()
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
                    tracing::warn!("the Aaronia RTSA API is unavailable: {error}");
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

pub fn shared() -> Result<Arc<dyn Rtsa>, String> {
    if !platform_supported() {
        return Err("Aaronia ships no RTSA-Suite for this platform".to_string());
    }
    let mut loader = LOADER.lock().unwrap_or_else(PoisonError::into_inner);
    let Loader { api, failure } = &mut *loader;
    Loader::resolve_with(api, failure, Instant::now(), load).map(|api| api as Arc<dyn Rtsa>)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_busy_device_reads_as_in_use() {
        assert!(matches!(
            check(ffi::ERROR_BUSY, "opening"),
            Err(DeviceError::InUse(_))
        ));
    }

    #[test]
    fn a_rejected_value_reads_as_unsupported() {
        assert!(matches!(
            check(ffi::ERROR_VALUE_INVALID, "main/centerfreq"),
            Err(DeviceError::Unsupported(text)) if text.contains("main/centerfreq")
        ));
    }

    #[test]
    fn states_and_warnings_are_not_errors() {
        for code in [ffi::OK, ffi::EMPTY, ffi::RUNNING, 0x4000_0001] {
            assert!(check(code, "anything").is_ok(), "0x{code:08x}");
        }
    }

    #[test]
    fn a_missing_paths_file_names_the_override() {
        let error = check(ffi::ERROR_MISSING_PATHS_FILE, "initialising").expect_err("missing");
        assert!(error.to_string().contains(OVERRIDE_VAR));
    }

    #[test]
    fn options_split_on_semicolons_without_empty_entries() {
        assert_eq!(
            split_options("Rx1;Rx2; auto;;"),
            vec!["Rx1".to_string(), "Rx2".to_string(), "auto".to_string()]
        );
    }

    #[test]
    fn an_install_names_the_library_inside_it() {
        let dir = Path::new("/somewhere");
        let installs: Vec<Install> = libraries_in(dir).collect();
        assert!(!installs.is_empty());
        assert!(
            installs
                .iter()
                .all(|install| install.library.starts_with(dir))
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

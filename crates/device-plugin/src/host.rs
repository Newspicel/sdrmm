//! The host's side: loads a plugin library and wraps its table back into a [`DeviceDriver`].

mod device;
mod sink;

use std::{path::Path, sync::OnceLock};

use sdrmm_device::{DeviceDriver, DeviceError};
use sdrmm_wire::BackendSpec;
use serde::de::DeserializeOwned;

use crate::abi::{
    Bytes, DEVICE_ABI_VERSION, ENTRY_SYMBOL, Entry, HostV1, PluginV1, Status, Str, decode_error,
};

pub use device::PluginDriver;

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("not a loadable library: {0}")]
    Open(String),
    #[error("not a device plugin: it exports no {ENTRY_SYMBOL}")]
    Symbol,
    #[error("the plugin refused to start; its log says why")]
    Refused,
    #[error("built for device plugin ABI {found}, this sdr-- speaks {DEVICE_ABI_VERSION}")]
    Abi { found: u32 },
    #[error("the plugin's spec is unreadable: {0}")]
    Spec(String),
    #[error("built with panic = \"abort\", so one panic would stop the server; build it to unwind")]
    PanicAbort,
}

/// A plugin that loaded.
pub struct Loaded {
    pub driver: Box<dyn DeviceDriver>,
    pub spec: BackendSpec,
    /// The plugin's crate name and version.
    pub plugin: String,
    /// The sdr-- revision it was built against, or empty.
    pub sdk_build: String,
}

/// Loads the device plugin at `path`. The library stays loaded until the process exits, since
/// its threads and the host's handles into it may outlive any one driver.
pub fn load(path: &Path) -> Result<Loaded, LoadError> {
    let library = unsafe { libloading::Library::new(path) }
        .map_err(|error| LoadError::Open(error.to_string()))?;
    let entry =
        unsafe { library.get::<Entry>(ENTRY_SYMBOL.as_bytes()) }.map_err(|_| LoadError::Symbol)?;
    let table = unsafe { entry(host_table()) };
    std::mem::forget(library);
    if table.is_null() {
        return Err(LoadError::Refused);
    }
    let [abi, size] = unsafe { table.cast::<[u32; 2]>().read() };
    if abi != DEVICE_ABI_VERSION || (size as usize) < size_of::<PluginV1>() {
        return Err(LoadError::Abi { found: abi });
    }
    from_table(unsafe { &*table })
}

/// Wraps a table already in this process. [`load`] ends here; tests start here.
pub fn from_table(table: &'static PluginV1) -> Result<Loaded, LoadError> {
    if table.abi != DEVICE_ABI_VERSION || (table.size as usize) < size_of::<PluginV1>() {
        return Err(LoadError::Abi { found: table.abi });
    }
    if !table.panic_unwind {
        return Err(LoadError::PanicAbort);
    }
    let spec: BackendSpec = json(table, |out| (table.spec)(table.driver, out))
        .map_err(|error| LoadError::Spec(error.to_string()))?;
    Ok(Loaded {
        driver: Box::new(PluginDriver::new(table, spec.clone())),
        spec,
        plugin: unsafe { table.plugin.text() }.into_owned(),
        sdk_build: unsafe { table.sdk_build.text() }.into_owned(),
    })
}

/// The table every plugin is lent, built once with the log level the host keeps.
pub fn host_table() -> &'static HostV1 {
    static HOST: OnceLock<HostV1> = OnceLock::new();
    HOST.get_or_init(|| HostV1 {
        abi: DEVICE_ABI_VERSION,
        size: size_of::<HostV1>() as u32,
        max_level: max_level(),
        log,
        sink_samples: sink::samples,
        sink_event: sink::event,
        sink_dropped: sink::dropped,
        sink_fail: sink::fail,
        sink_release: sink::release,
        sweep_push: sink::sweep_push,
        sweep_fail: sink::sweep_fail,
        sweep_release: sink::sweep_release,
    })
}

fn max_level() -> u32 {
    use tracing::level_filters::LevelFilter;
    match LevelFilter::current() {
        LevelFilter::OFF => 0,
        LevelFilter::ERROR => 1,
        LevelFilter::WARN => 2,
        LevelFilter::INFO => 3,
        LevelFilter::DEBUG => 4,
        _ => 5,
    }
}

extern "C" fn log(level: u32, target: Str, message: Str) {
    let _ = std::panic::catch_unwind(|| {
        let from = unsafe { target.text() };
        let message = unsafe { message.text() };
        match level {
            1 => tracing::error!(target: "device_plugin", %from, "{message}"),
            2 => tracing::warn!(target: "device_plugin", %from, "{message}"),
            3 => tracing::info!(target: "device_plugin", %from, "{message}"),
            4 => tracing::debug!(target: "device_plugin", %from, "{message}"),
            _ => tracing::trace!(target: "device_plugin", %from, "{message}"),
        }
    });
}

pub(crate) fn status(table: &PluginV1, status: Status) -> Result<(), DeviceError> {
    let text = take(table, status.message);
    if status.code == crate::abi::OK {
        return Ok(());
    }
    Err(decode_error(
        status.code,
        String::from_utf8_lossy(&text).into_owned(),
    ))
}

pub(crate) fn take(table: &PluginV1, bytes: Bytes) -> Vec<u8> {
    let copy = unsafe { bytes.as_slice() }.to_vec();
    (table.free_bytes)(bytes);
    copy
}

pub(crate) fn json<T: DeserializeOwned>(
    table: &PluginV1,
    call: impl FnOnce(*mut Bytes) -> Status,
) -> Result<T, DeviceError> {
    let mut out = Bytes::EMPTY;
    let result = status(table, call(&raw mut out));
    let answer = take(table, out);
    result?;
    serde_json::from_slice(&answer)
        .map_err(|error| DeviceError::Io(format!("plugin answered with unreadable JSON: {error}")))
}

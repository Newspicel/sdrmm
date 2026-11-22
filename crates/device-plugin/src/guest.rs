//! The plugin's side: turns a [`DeviceDriver`] into the C table the host loads.

mod device;
mod log;
mod sink;

use std::panic::{AssertUnwindSafe, catch_unwind};

use sdrmm_device::{DeviceDriver, DeviceError};
use serde::{Serialize, de::DeserializeOwned};

use crate::abi::{Bytes, DEVICE_ABI_VERSION, HostV1, PluginV1, Status, Str};

pub use log::Forwarder;

/// Exports `$driver` as this library's device plugin. Call it once, in the crate built as a
/// `cdylib`.
#[macro_export]
macro_rules! export_driver {
    ($driver:expr) => {
        /// # Safety
        /// Called once by the sdr-- host with its own static table.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn sdrmm_device_plugin_v1(
            host: *const $crate::abi::HostV1,
        ) -> *const $crate::abi::PluginV1 {
            unsafe {
                $crate::guest::entry(
                    host,
                    concat!(env!("CARGO_PKG_NAME"), " ", env!("CARGO_PKG_VERSION")),
                    || Box::new($driver),
                    true,
                )
            }
        }
    };
}

pub(crate) struct GuestDriver {
    pub(crate) driver: Box<dyn DeviceDriver>,
    pub(crate) host: &'static HostV1,
}

/// `host` must point at a table the host keeps for the life of the process.
///
/// Builds the table for `make`'s driver, or returns null when the host speaks another ABI or the
/// driver panics while it is made. `log` installs the forwarding logger, which only a real
/// plugin library wants.
///
/// # Safety
/// `host` is null or points at a valid [`HostV1`] that lives for the rest of the process.
#[doc(hidden)]
pub unsafe fn entry(
    host: *const HostV1,
    plugin: &'static str,
    make: fn() -> Box<dyn DeviceDriver>,
    log: bool,
) -> *const PluginV1 {
    let Some(host) = (unsafe { host.as_ref::<'static>() }) else {
        return std::ptr::null();
    };
    if host.abi != DEVICE_ABI_VERSION || (host.size as usize) < size_of::<HostV1>() {
        let message = format!(
            "{plugin} speaks device plugin ABI {DEVICE_ABI_VERSION}, the host ABI {}: rebuild it",
            host.abi
        );
        (host.log)(1, Str::of(plugin.as_bytes()), Str::of(message.as_bytes()));
        return std::ptr::null();
    }
    if log {
        log::install(host);
    }
    let driver = match catch_unwind(make) {
        Ok(driver) => driver,
        Err(payload) => {
            let message = format!(
                "{plugin} panicked while starting: {}",
                panic_text(&*payload)
            );
            (host.log)(1, Str::of(plugin.as_bytes()), Str::of(message.as_bytes()));
            return std::ptr::null();
        }
    };
    let driver = Box::into_raw(Box::new(GuestDriver { driver, host }));
    std::ptr::from_ref(Box::leak(Box::new(device::table(plugin, driver.cast()))))
}

pub(crate) fn guard(call: impl FnOnce() -> Result<(), DeviceError>) -> Status {
    match catch_unwind(AssertUnwindSafe(call)) {
        Ok(Ok(())) => Status::OK,
        Ok(Err(error)) => Status::from_error(&error),
        Err(payload) => Status::from_error(&DeviceError::Io(format!(
            "plugin panicked: {}",
            panic_text(&*payload)
        ))),
    }
}

pub(crate) fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "no message".to_owned())
}

pub(crate) fn put_json<T: Serialize>(out: *mut Bytes, value: &T) -> Result<(), DeviceError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| DeviceError::Io(format!("plugin could not encode its answer: {error}")))?;
    unsafe { out.write(Bytes::from_vec(bytes)) };
    Ok(())
}

pub(crate) fn take_json<T: DeserializeOwned>(text: Str) -> Result<T, DeviceError> {
    serde_json::from_slice(unsafe { text.bytes() })
        .map_err(|error| DeviceError::Io(format!("plugin could not read the host's call: {error}")))
}

pub(crate) extern "C" fn free_bytes(bytes: Bytes) {
    drop(unsafe { bytes.into_vec() });
}

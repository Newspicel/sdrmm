use std::time::Duration;

use crate::protocol::Command;

pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("invalid configuration for {field}: {reason}")]
    InvalidConfig {
        field: &'static str,
        reason: &'static str,
    },

    #[error("no matching FUNcube Dongle Pro+ found")]
    DeviceNotFound,

    #[error("{operation}: no reply within {timeout:?}")]
    Timeout {
        operation: &'static str,
        timeout: Duration,
    },

    #[error("the firmware refused to {}", .0.name())]
    Rejected(Command),

    #[error("{operation}: {reason}")]
    Protocol {
        operation: &'static str,
        reason: &'static str,
    },

    #[error("listing USB devices: {0}")]
    Usb(#[source] nusb::Error),

    #[cfg(target_os = "macos")]
    #[error("the FUNcube Dongle Pro+ has no audio device in Core Audio")]
    AudioNotFound,

    #[cfg(target_os = "macos")]
    #[error("{operation}: IOKit error {code:#x}")]
    Hid { operation: &'static str, code: i32 },

    #[cfg(target_os = "macos")]
    #[error("{operation}: Core Audio error {status}")]
    Audio {
        operation: &'static str,
        status: i32,
    },

    #[cfg(target_os = "macos")]
    #[error("macOS delivers silence: allow Microphone access for this app in System Settings")]
    Silenced,

    #[cfg(target_os = "linux")]
    #[error("{operation}: {errno}")]
    Usbfs {
        operation: &'static str,
        errno: rustix::io::Errno,
    },

    #[cfg(target_os = "windows")]
    #[error("{operation}: Windows error {code}")]
    Win32 { operation: &'static str, code: u32 },

    #[cfg(target_os = "windows")]
    #[error(
        "the FUNcube Dongle Pro+ needs the WinUSB driver: install it with Zadig on its composite parent"
    )]
    NeedsWinUsb,
}

impl Error {
    pub(crate) const fn invalid_config(field: &'static str, reason: &'static str) -> Self {
        Self::InvalidConfig { field, reason }
    }

    pub(crate) const fn protocol(operation: &'static str, reason: &'static str) -> Self {
        Self::Protocol { operation, reason }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn hid(operation: &'static str, code: i32) -> Self {
        Self::Hid { operation, code }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn usbfs(operation: &'static str) -> impl FnOnce(rustix::io::Errno) -> Self {
        move |errno| Self::Usbfs { operation, errno }
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn win32(operation: &'static str, code: u32) -> Self {
        Self::Win32 { operation, code }
    }

    pub(crate) fn is_disconnected(&self) -> bool {
        match self {
            Self::Usb(source) => source.kind() == nusb::ErrorKind::Disconnected,
            #[cfg(target_os = "macos")]
            Self::Hid { code, .. } => {
                let code = code.cast_unsigned();
                code == objc2_io_kit::kIOReturnNoDevice
                    || code == objc2_io_kit::kIOReturnNotAttached
            }
            #[cfg(target_os = "linux")]
            Self::Usbfs { errno, .. } => {
                use rustix::io::Errno;
                [Errno::NODEV, Errno::SHUTDOWN, Errno::NOENT].contains(errno)
            }
            #[cfg(target_os = "windows")]
            Self::Win32 { code, .. } => {
                use windows_sys::Win32::Foundation::{
                    ERROR_BAD_COMMAND, ERROR_DEVICE_NOT_CONNECTED, ERROR_FILE_NOT_FOUND,
                };
                [
                    ERROR_DEVICE_NOT_CONNECTED,
                    ERROR_BAD_COMMAND,
                    ERROR_FILE_NOT_FOUND,
                ]
                .contains(code)
            }
            _ => false,
        }
    }

    pub(crate) fn is_busy(&self) -> bool {
        match self {
            #[cfg(target_os = "macos")]
            Self::Hid { code, .. } => {
                code.cast_unsigned() == objc2_io_kit::kIOReturnExclusiveAccess
            }
            #[cfg(target_os = "linux")]
            Self::Usbfs { errno, .. } => *errno == rustix::io::Errno::BUSY,
            #[cfg(target_os = "windows")]
            Self::Win32 { code, .. } => {
                *code == windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED
            }
            _ => false,
        }
    }

    pub(crate) fn is_permission_denied(&self) -> bool {
        match self {
            Self::Usb(source) => source.kind() == nusb::ErrorKind::PermissionDenied,
            #[cfg(target_os = "macos")]
            Self::Silenced => true,
            #[cfg(target_os = "linux")]
            Self::Usbfs { errno, .. } => {
                [rustix::io::Errno::ACCESS, rustix::io::Errno::PERM].contains(errno)
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_names_what_was_refused() {
        assert_eq!(
            Error::Rejected(Command::SetBiasTee).to_string(),
            "the firmware refused to set bias tee"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_vanished_device_reads_as_disconnected() {
        use objc2_io_kit::{kIOReturnExclusiveAccess, kIOReturnNoDevice, kIOReturnNotAttached};
        assert!(Error::hid("set report", kIOReturnNoDevice.cast_signed()).is_disconnected());
        assert!(Error::hid("set report", kIOReturnNotAttached.cast_signed()).is_disconnected());
        assert!(!Error::hid("set report", 0).is_disconnected());
        assert!(Error::hid("open", kIOReturnExclusiveAccess.cast_signed()).is_busy());
        assert!(Error::Silenced.is_permission_denied());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn errno_decides_what_the_engine_hears() {
        use rustix::io::Errno;
        assert!(Error::usbfs("reap")(Errno::NODEV).is_disconnected());
        assert!(Error::usbfs("claim")(Errno::BUSY).is_busy());
        assert!(Error::usbfs("open")(Errno::ACCESS).is_permission_denied());
        assert!(!Error::usbfs("submit")(Errno::INVAL).is_disconnected());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn win32_codes_decide_what_the_engine_hears() {
        use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_DEVICE_NOT_CONNECTED};
        assert!(Error::win32("read", ERROR_DEVICE_NOT_CONNECTED).is_disconnected());
        assert!(Error::win32("open", ERROR_ACCESS_DENIED).is_busy());
        assert!(!Error::NeedsWinUsb.is_disconnected());
    }
}

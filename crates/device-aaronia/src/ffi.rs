//! The RTSA API's C interface, declared from its public SDK interface description and held to the
//! layouts it documents by compile-time size assertions.

use std::ffi::{c_int, c_void};

/// `wchar_t`: two bytes of UTF-16 on Windows, four bytes of a code point everywhere else.
#[cfg(windows)]
pub type WChar = u16;
#[cfg(not(windows))]
pub type WChar = u32;

pub type ResultCode = u32;

pub const OK: ResultCode = 0x0000_0000;
pub const EMPTY: ResultCode = 0x0000_0001;
pub const RETRY: ResultCode = 0x0000_0002;
pub const RUNNING: ResultCode = 0x1000_0004;
pub const ERROR: ResultCode = 0x8000_0000;
pub const ERROR_NOT_FOUND: ResultCode = 0x8000_0002;
pub const ERROR_BUSY: ResultCode = 0x8000_0003;
pub const ERROR_NOT_CONNECTED: ResultCode = 0x8000_0005;
pub const ERROR_INVALID_CONFIG: ResultCode = 0x8000_0006;
pub const ERROR_INVALID_PARAMETER: ResultCode = 0x8000_0009;
pub const ERROR_MISSING_PATHS_FILE: ResultCode = 0x8000_000b;
pub const ERROR_VALUE_INVALID: ResultCode = 0x8000_000c;
pub const ERROR_VALUE_MALFORMED: ResultCode = 0x8000_000d;

pub const MEMORY_MEDIUM: u32 = 1;

pub const PACKET_STREAM_START: u64 = 0x0001;
pub const PACKET_STREAM_END: u64 = 0x0002;
pub const PACKET_SEGMENT_START: u64 = 0x0004;
pub const PACKET_SEGMENT_END: u64 = 0x0008;
pub const PACKET_WARN_OVERFLOW: u64 = 0x0100;
pub const PACKET_WARN_DROPPED: u64 = 0x0200;

pub const CONFIG_TYPE_NUMBER: c_int = 3;
pub const CONFIG_TYPE_BOOL: c_int = 4;
pub const CONFIG_TYPE_ENUM: c_int = 5;
pub const CONFIG_TYPE_STRING: c_int = 6;

pub const SERIAL_LEN: usize = 120;
pub const NAME_LEN: usize = 80;
pub const TITLE_LEN: usize = 120;
pub const UNIT_LEN: usize = 10;
pub const OPTIONS_LEN: usize = 1000;
pub const STRING_LEN: usize = 1000;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Handle {
    pub d: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Device {
    pub d: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub d: *mut c_void,
}

impl Default for Handle {
    fn default() -> Self {
        Self {
            d: std::ptr::null_mut(),
        }
    }
}

impl Default for Device {
    fn default() -> Self {
        Self {
            d: std::ptr::null_mut(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            d: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Packet {
    pub cbsize: i64,
    pub stream_id: u64,
    pub flags: u64,
    pub start_time: f64,
    pub end_time: f64,
    pub start_frequency: f64,
    pub step_frequency: f64,
    pub span_frequency: f64,
    pub rbw_frequency: f64,
    pub num: i64,
    pub total: i64,
    pub size: i64,
    pub stride: i64,
    pub fp32: *mut f32,
    pub interleave: i64,
    pub time_base: f64,
    pub start_time_offset: f64,
    pub end_time_offset: f64,
}

impl Default for Packet {
    fn default() -> Self {
        Self {
            cbsize: size_of::<Self>() as i64,
            stream_id: 0,
            flags: 0,
            start_time: 0.0,
            end_time: 0.0,
            start_frequency: 0.0,
            step_frequency: 0.0,
            span_frequency: 0.0,
            rbw_frequency: 0.0,
            num: 0,
            total: 0,
            size: 0,
            stride: 0,
            fp32: std::ptr::null_mut(),
            interleave: 0,
            time_base: 0.0,
            start_time_offset: 0.0,
            end_time_offset: 0.0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DeviceInfo {
    pub cbsize: i64,
    pub serial_number: [WChar; SERIAL_LEN],
    pub ready: u8,
    pub boost: u8,
    pub superspeed: u8,
    pub active: u8,
}

impl Default for DeviceInfo {
    fn default() -> Self {
        Self {
            cbsize: size_of::<Self>() as i64,
            serial_number: [0; SERIAL_LEN],
            ready: 0,
            boost: 0,
            superspeed: 0,
            active: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ConfigInfo {
    pub cbsize: i64,
    pub name: [WChar; NAME_LEN],
    pub title: [WChar; TITLE_LEN],
    pub kind: c_int,
    pub min_value: f64,
    pub max_value: f64,
    pub step_value: f64,
    pub unit: [WChar; UNIT_LEN],
    pub options: [WChar; OPTIONS_LEN],
    pub disabled_options: u64,
}

impl Default for ConfigInfo {
    fn default() -> Self {
        Self {
            cbsize: size_of::<Self>() as i64,
            name: [0; NAME_LEN],
            title: [0; TITLE_LEN],
            kind: 0,
            min_value: 0.0,
            max_value: 0.0,
            step_value: 0.0,
            unit: [0; UNIT_LEN],
            options: [0; OPTIONS_LEN],
            disabled_options: 0,
        }
    }
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<Packet>() == 144);
#[cfg(all(target_pointer_width = "64", not(windows)))]
const _: () = {
    assert!(size_of::<DeviceInfo>() == 496);
    assert!(size_of::<ConfigInfo>() == 4888);
};
#[cfg(all(target_pointer_width = "64", windows))]
const _: () = {
    assert!(size_of::<DeviceInfo>() == 256);
    assert!(size_of::<ConfigInfo>() == 2472);
};

pub type InitWithPathFn = unsafe extern "C" fn(u32, *const WChar) -> ResultCode;
pub type ShutdownFn = unsafe extern "C" fn() -> ResultCode;
pub type VersionFn = unsafe extern "C" fn() -> u32;
pub type OpenFn = unsafe extern "C" fn(*mut Handle) -> ResultCode;
pub type CloseFn = unsafe extern "C" fn(*mut Handle) -> ResultCode;
pub type RescanFn = unsafe extern "C" fn(*mut Handle, c_int) -> ResultCode;
pub type EnumDeviceFn =
    unsafe extern "C" fn(*mut Handle, *const WChar, i32, *mut DeviceInfo) -> ResultCode;
pub type OpenDeviceFn =
    unsafe extern "C" fn(*mut Handle, *mut Device, *const WChar, *const WChar) -> ResultCode;
pub type CloseDeviceFn = unsafe extern "C" fn(*mut Handle, *mut Device) -> ResultCode;
pub type DeviceFn = unsafe extern "C" fn(*mut Device) -> ResultCode;
pub type GetPacketFn = unsafe extern "C" fn(*mut Device, i32, i32, *mut Packet) -> ResultCode;
pub type ConsumeFn = unsafe extern "C" fn(*mut Device, i32, i32) -> ResultCode;
pub type StreamTimeFn = unsafe extern "C" fn(*mut Device, *mut f64) -> ResultCode;
pub type SendPacketFn = unsafe extern "C" fn(*mut Device, i32, *const Packet) -> ResultCode;
pub type ConfigRootFn = unsafe extern "C" fn(*mut Device, *mut Config) -> ResultCode;
pub type ConfigFindFn =
    unsafe extern "C" fn(*mut Device, *mut Config, *mut Config, *const WChar) -> ResultCode;
pub type ConfigInfoFn =
    unsafe extern "C" fn(*mut Device, *mut Config, *mut ConfigInfo) -> ResultCode;
pub type SetFloatFn = unsafe extern "C" fn(*mut Device, *mut Config, f64) -> ResultCode;
pub type GetFloatFn = unsafe extern "C" fn(*mut Device, *mut Config, *mut f64) -> ResultCode;
pub type SetStringFn = unsafe extern "C" fn(*mut Device, *mut Config, *const WChar) -> ResultCode;
pub type GetStringFn =
    unsafe extern "C" fn(*mut Device, *mut Config, *mut WChar, *mut i64) -> ResultCode;

/// `text` as a NUL-terminated `wchar_t` string.
#[must_use]
pub fn wide(text: &str) -> Vec<WChar> {
    #[cfg(windows)]
    let mut out: Vec<WChar> = text.encode_utf16().collect();
    #[cfg(not(windows))]
    let mut out: Vec<WChar> = text.chars().map(u32::from).collect();
    out.push(0);
    out
}

/// A `wchar_t` buffer up to its first NUL, or all of it when it fills the array.
#[must_use]
pub fn narrow(buffer: &[WChar]) -> String {
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    let text = &buffer[..end];
    #[cfg(windows)]
    return String::from_utf16_lossy(text);
    #[cfg(not(windows))]
    text.iter()
        .map(|&c| char::from_u32(c).unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_string_round_trips_and_ends_at_its_terminator() {
        let encoded = wide("spectranv6/iqreceiver");
        assert_eq!(encoded.last(), Some(&0));
        assert_eq!(narrow(&encoded), "spectranv6/iqreceiver");
        assert_eq!(narrow(&wide("Ä µ")), "Ä µ");
    }

    #[test]
    fn a_buffer_without_a_terminator_is_read_to_its_end_and_no_further() {
        let full: Vec<WChar> = wide("ABCD").into_iter().take(4).collect();
        assert_eq!(narrow(&full), "ABCD");
    }

    #[test]
    fn every_size_field_names_its_own_structure() {
        assert_eq!(Packet::default().cbsize, size_of::<Packet>() as i64);
        assert_eq!(DeviceInfo::default().cbsize, size_of::<DeviceInfo>() as i64);
        assert_eq!(ConfigInfo::default().cbsize, size_of::<ConfigInfo>() as i64);
    }
}

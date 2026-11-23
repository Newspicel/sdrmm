//! The RSA API's C interface, declared from its public programmer manual and held to the layouts
//! it documents by compile-time size assertions.

use std::ffi::{c_char, c_int, c_void};

pub type ReturnStatus = c_int;

pub const NO_ERROR: ReturnStatus = 0;
pub const ERROR_NOT_CONNECTED: ReturnStatus = 101;
pub const ERROR_PARAMETER: ReturnStatus = 302;
pub const ERROR_DATA_NOT_READY: ReturnStatus = 303;
pub const ERROR_FREQUENCY_OUT_OF_RANGE: ReturnStatus = 1104;
pub const ERROR_IQ_STREAM_BANDWIDTH_OUT_OF_RANGE: ReturnStatus = 1303;
pub const ERROR_IQ_STREAMING_NOT_ENABLED: ReturnStatus = 1304;
pub const ERROR_TRANSFER: ReturnStatus = 3002;
pub const ERROR_DISCONNECTED_FIRST: ReturnStatus = 3101;
pub const ERROR_DISCONNECTED_LAST: ReturnStatus = 3199;

pub const SEARCH_MAX_DEVICES: usize = 20;
pub const SEARCH_SERIAL_LEN: usize = 100;
pub const SEARCH_TYPE_LEN: usize = 20;
pub const INFO_LEN: usize = 100;

pub const OUTPUT_CLIENT: c_int = 0;
pub const OUTPUT_SINGLE: c_int = 0;

pub const SOURCE_INTERNAL: c_int = 0;
pub const SOURCE_EXTERNAL: c_int = 1;
pub const SOURCE_GNSS: c_int = 2;

pub const STATUS_OVERRANGE: u32 = 1 << 0;
pub const STATUS_TRANSFER_GAP: u32 = 1 << 1;
pub const STATUS_INPUT_OVERFLOW: u32 = 1 << 3;
pub const STATUS_OUTPUT_OVERFLOW: u32 = 1 << 5;
pub const STATUS_LOSS: u32 = STATUS_TRANSFER_GAP | STATUS_INPUT_OVERFLOW | STATUS_OUTPUT_OVERFLOW;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct IqInfo {
    pub timestamp: u64,
    pub trigger_count: c_int,
    pub trigger_indices: *mut c_int,
    pub scale_factor: f64,
    pub acq_status: u32,
}

impl Default for IqInfo {
    fn default() -> Self {
        Self {
            timestamp: 0,
            trigger_count: 0,
            trigger_indices: std::ptr::null_mut(),
            scale_factor: 0.0,
            acq_status: 0,
        }
    }
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<IqInfo>() == 40);

pub type SerialBuffer = [[c_char; SEARCH_SERIAL_LEN]; SEARCH_MAX_DEVICES];
pub type TypeBuffer = [[c_char; SEARCH_TYPE_LEN]; SEARCH_MAX_DEVICES];

pub type ErrorStringFn = unsafe extern "C" fn(ReturnStatus) -> *const c_char;
pub type SearchFn = unsafe extern "C" fn(
    *mut c_int,
    *mut c_int,
    *mut [c_char; SEARCH_SERIAL_LEN],
    *mut [c_char; SEARCH_TYPE_LEN],
) -> ReturnStatus;
pub type ConnectFn = unsafe extern "C" fn(c_int) -> ReturnStatus;
pub type PlainFn = unsafe extern "C" fn() -> ReturnStatus;
pub type TextFn = unsafe extern "C" fn(*mut c_char) -> ReturnStatus;
pub type SetDoubleFn = unsafe extern "C" fn(f64) -> ReturnStatus;
pub type GetDoubleFn = unsafe extern "C" fn(*mut f64) -> ReturnStatus;
pub type GetTwoDoublesFn = unsafe extern "C" fn(*mut f64, *mut f64) -> ReturnStatus;
pub type SetBoolFn = unsafe extern "C" fn(bool) -> ReturnStatus;
pub type GetBoolFn = unsafe extern "C" fn(*mut bool) -> ReturnStatus;
pub type SetIntFn = unsafe extern "C" fn(c_int) -> ReturnStatus;
pub type GetIntFn = unsafe extern "C" fn(*mut c_int) -> ReturnStatus;
pub type GetU64Fn = unsafe extern "C" fn(*mut u64) -> ReturnStatus;
pub type OutputFn = unsafe extern "C" fn(c_int, c_int) -> ReturnStatus;
pub type WaitFn = unsafe extern "C" fn(c_int, *mut bool) -> ReturnStatus;
pub type IqDataFn = unsafe extern "C" fn(*mut c_void, *mut c_int, *mut IqInfo) -> ReturnStatus;

/// A `char` buffer up to its first NUL, or all of it when it fills the array.
#[must_use]
pub fn text(buffer: &[c_char]) -> String {
    let bytes: Vec<u8> = buffer
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c.cast_unsigned())
        .collect();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(text: &[u8]) -> Vec<c_char> {
        text.iter().map(|&b| b.cast_signed()).collect()
    }

    #[test]
    fn text_ends_at_the_terminator() {
        assert_eq!(text(&buffer(b"RSA306\0junk")), "RSA306");
    }

    #[test]
    fn a_buffer_without_a_terminator_is_read_to_its_end_and_no_further() {
        assert_eq!(text(&buffer(b"B010")), "B010");
    }
}

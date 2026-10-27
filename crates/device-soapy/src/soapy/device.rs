use std::{
    ffi::{CString, c_char, c_int, c_void},
    sync::Arc,
};

use super::{
    api::{self, Library},
    args::Args,
    ffi,
    stream::{RxStream, StreamSample, TxStream},
    types::{ArgInfo, Direction, Error, ErrorCode, Range, arg_info, required_string},
};

/// SoapySDR requires every device method implementation to be thread safe, which is what makes
/// the `Send` and `Sync` implementations below sound.
struct Inner {
    handle: ffi::Device,
    library: Arc<Library>,
}

unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

impl Drop for Inner {
    fn drop(&mut self) {
        unsafe { (self.library.entries.unmake)(self.handle) };
    }
}

struct Owned<T, F: FnMut(*mut T, usize)> {
    pointer: *mut T,
    length: usize,
    free: F,
}

impl<T, F: FnMut(*mut T, usize)> Owned<T, F> {
    const fn new(pointer: *mut T, length: usize, free: F) -> Self {
        Self {
            pointer,
            length,
            free,
        }
    }
}

impl<T, F: FnMut(*mut T, usize)> Drop for Owned<T, F> {
    fn drop(&mut self) {
        if !self.pointer.is_null() {
            (self.free)(self.pointer, self.length);
        }
    }
}

struct Clear<T, F: FnMut(*mut T)> {
    held: *mut T,
    clear: F,
}

impl<T, F: FnMut(*mut T)> Clear<T, F> {
    const fn new(held: *mut T, clear: F) -> Self {
        Self { held, clear }
    }

    const fn held(&mut self) -> &mut T {
        unsafe { &mut *self.held }
    }
}

impl<T, F: FnMut(*mut T)> Drop for Clear<T, F> {
    fn drop(&mut self) {
        (self.clear)(self.held);
    }
}

#[derive(Clone)]
pub struct Device {
    inner: Arc<Inner>,
}

impl Device {
    pub fn new(args: impl Into<Args>) -> Result<Self, Error> {
        let library = api::shared().map_err(Error::other)?;
        let markup = cstring(&args.into().to_string())?;
        let handle = unsafe { (library.entries.make)(markup.as_ptr()) };
        if handle.is_null() {
            return Err(Error::other(last_error(&library)));
        }
        Ok(Self {
            inner: Arc::new(Inner { handle, library }),
        })
    }

    pub(crate) fn library(&self) -> &Arc<Library> {
        &self.inner.library
    }

    pub(crate) fn handle(&self) -> ffi::Device {
        self.inner.handle
    }

    fn entries(&self) -> &api::Entries {
        &self.inner.library.entries
    }

    /// Reports whether the last call threw, for the calls that have no integer status to return.
    ///
    /// The C bindings set `lastStatus` to -1 for every caught exception, which is also
    /// `SOAPY_SDR_TIMEOUT`, so the status carries no code worth reading, only the message does.
    /// Reading it as a stream code would let a driver fault pass for a read timeout, which the
    /// capture loop retries instead of failing.
    pub(crate) fn check<T>(&self, value: T) -> Result<T, Error> {
        if unsafe { (self.entries().last_status)() } == 0 {
            return Ok(value);
        }
        Err(Error::other(last_error(&self.inner.library)))
    }

    pub(crate) fn returned(&self, code: c_int) -> Result<(), Error> {
        if code == 0 {
            return Ok(());
        }
        Err(self.failure(code))
    }

    pub(crate) fn failure(&self, code: c_int) -> Error {
        Error {
            code: ErrorCode::from_raw(code),
            message: last_error(&self.inner.library),
        }
    }

    fn string(&self, pointer: *mut c_char) -> Result<String, Error> {
        let entries = self.entries();
        let owned = Owned::new(pointer, 1, |pointer, _| unsafe {
            (entries.free)(pointer.cast::<c_void>());
        });
        self.check(())?;
        if owned.pointer.is_null() {
            return Ok(String::new());
        }
        Ok(unsafe { required_string(owned.pointer) })
    }

    fn strings(
        &self,
        call: impl FnOnce(*mut usize) -> *mut *mut c_char,
    ) -> Result<Vec<String>, Error> {
        let mut length: usize = 0;
        let entries = self.entries();
        let owned = Owned::new(
            call(&raw mut length),
            length,
            |mut pointer, length| unsafe {
                (entries.strings_clear)(&raw mut pointer, length);
            },
        );
        self.check(())?;
        if owned.pointer.is_null() {
            return Ok(Vec::new());
        }
        Ok(unsafe { std::slice::from_raw_parts(owned.pointer, length) }
            .iter()
            .map(|&text| unsafe { required_string(text) })
            .collect())
    }

    fn ranges(&self, call: impl FnOnce(*mut usize) -> *mut Range) -> Result<Vec<Range>, Error> {
        let mut length: usize = 0;
        let entries = self.entries();
        let owned = Owned::new(call(&raw mut length), length, |pointer, _| unsafe {
            (entries.free)(pointer.cast::<c_void>());
        });
        self.check(())?;
        if owned.pointer.is_null() {
            return Ok(Vec::new());
        }
        Ok(unsafe { std::slice::from_raw_parts(owned.pointer, length) }.to_vec())
    }

    fn arg_infos(
        &self,
        call: impl FnOnce(*mut usize) -> *mut ffi::ArgInfo,
    ) -> Result<Vec<ArgInfo>, Error> {
        let mut length: usize = 0;
        let entries = self.entries();
        let owned = Owned::new(call(&raw mut length), length, |pointer, length| unsafe {
            (entries.arg_info_list_clear)(pointer, length);
        });
        self.check(())?;
        if owned.pointer.is_null() {
            return Ok(Vec::new());
        }
        Ok(unsafe { std::slice::from_raw_parts(owned.pointer, length) }
            .iter()
            .map(|raw| unsafe { arg_info(raw) })
            .collect())
    }

    fn kwargs(&self, raw: ffi::Kwargs) -> Result<Args, Error> {
        let mut raw = raw;
        let entries = self.entries();
        let mut clear = Clear::new(&raw mut raw, |raw| unsafe {
            (entries.kwargs_clear)(raw);
        });
        self.check(())?;
        Ok(unsafe { Args::from_kwargs(clear.held()) })
    }

    pub fn num_channels(&self, direction: Direction) -> Result<usize, Error> {
        self.check(unsafe { (self.entries().num_channels)(self.handle(), direction.into()) })
    }

    pub fn full_duplex(&self, direction: Direction, channel: usize) -> Result<bool, Error> {
        self.check(unsafe {
            (self.entries().full_duplex)(self.handle(), direction.into(), channel)
        })
    }

    pub fn hardware_info(&self) -> Result<Args, Error> {
        self.kwargs(unsafe { (self.entries().hardware_info)(self.handle()) })
    }

    pub fn channel_info(&self, direction: Direction, channel: usize) -> Result<Args, Error> {
        self.kwargs(unsafe {
            (self.entries().channel_info)(self.handle(), direction.into(), channel)
        })
    }

    pub fn stream_formats(
        &self,
        direction: Direction,
        channel: usize,
    ) -> Result<Vec<String>, Error> {
        self.strings(|length| unsafe {
            (self.entries().stream_formats)(self.handle(), direction.into(), channel, length)
        })
    }

    pub fn native_stream_format(
        &self,
        direction: Direction,
        channel: usize,
    ) -> Result<(String, f64), Error> {
        let mut full_scale = 0.0;
        let format = unsafe {
            (self.entries().native_stream_format)(
                self.handle(),
                direction.into(),
                channel,
                &raw mut full_scale,
            )
        };
        Ok((self.string(format)?, full_scale))
    }

    pub fn stream_args_info(
        &self,
        direction: Direction,
        channel: usize,
    ) -> Result<Vec<ArgInfo>, Error> {
        self.arg_infos(|length| unsafe {
            (self.entries().stream_args_info)(self.handle(), direction.into(), channel, length)
        })
    }

    pub fn frequency_args_info(
        &self,
        direction: Direction,
        channel: usize,
    ) -> Result<Vec<ArgInfo>, Error> {
        self.arg_infos(|length| unsafe {
            (self.entries().frequency_args_info)(self.handle(), direction.into(), channel, length)
        })
    }

    pub fn channel_setting_info(
        &self,
        direction: Direction,
        channel: usize,
    ) -> Result<Vec<ArgInfo>, Error> {
        self.arg_infos(|length| unsafe {
            (self.entries().channel_setting_info)(self.handle(), direction.into(), channel, length)
        })
    }

    pub fn setting_info(&self) -> Result<Vec<ArgInfo>, Error> {
        self.arg_infos(|length| unsafe { (self.entries().setting_info)(self.handle(), length) })
    }

    pub fn antennas(&self, direction: Direction, channel: usize) -> Result<Vec<String>, Error> {
        self.strings(|length| unsafe {
            (self.entries().list_antennas)(self.handle(), direction.into(), channel, length)
        })
    }

    pub fn antenna(&self, direction: Direction, channel: usize) -> Result<String, Error> {
        self.string(unsafe { (self.entries().antenna)(self.handle(), direction.into(), channel) })
    }

    pub fn set_antenna(
        &self,
        direction: Direction,
        channel: usize,
        name: &str,
    ) -> Result<(), Error> {
        let name = cstring(name)?;
        self.returned(unsafe {
            (self.entries().set_antenna)(self.handle(), direction.into(), channel, name.as_ptr())
        })
    }

    pub fn list_gains(&self, direction: Direction, channel: usize) -> Result<Vec<String>, Error> {
        self.strings(|length| unsafe {
            (self.entries().list_gains)(self.handle(), direction.into(), channel, length)
        })
    }

    pub fn gain_element_range(
        &self,
        direction: Direction,
        channel: usize,
        name: &str,
    ) -> Result<Range, Error> {
        let name = cstring(name)?;
        self.check(unsafe {
            (self.entries().gain_element_range)(
                self.handle(),
                direction.into(),
                channel,
                name.as_ptr(),
            )
        })
    }

    pub fn gain_element(
        &self,
        direction: Direction,
        channel: usize,
        name: &str,
    ) -> Result<f64, Error> {
        let name = cstring(name)?;
        self.check(unsafe {
            (self.entries().gain_element)(self.handle(), direction.into(), channel, name.as_ptr())
        })
    }

    pub fn set_gain_element(
        &self,
        direction: Direction,
        channel: usize,
        name: &str,
        value_db: f64,
    ) -> Result<(), Error> {
        let name = cstring(name)?;
        self.returned(unsafe {
            (self.entries().set_gain_element)(
                self.handle(),
                direction.into(),
                channel,
                name.as_ptr(),
                value_db,
            )
        })
    }

    pub fn has_gain_mode(&self, direction: Direction, channel: usize) -> Result<bool, Error> {
        self.check(unsafe {
            (self.entries().has_gain_mode)(self.handle(), direction.into(), channel)
        })
    }

    pub fn gain_mode(&self, direction: Direction, channel: usize) -> Result<bool, Error> {
        self.check(unsafe { (self.entries().gain_mode)(self.handle(), direction.into(), channel) })
    }

    pub fn set_gain_mode(
        &self,
        direction: Direction,
        channel: usize,
        automatic: bool,
    ) -> Result<(), Error> {
        self.returned(unsafe {
            (self.entries().set_gain_mode)(self.handle(), direction.into(), channel, automatic)
        })
    }

    pub fn has_dc_offset_mode(&self, direction: Direction, channel: usize) -> Result<bool, Error> {
        self.check(unsafe {
            (self.entries().has_dc_offset_mode)(self.handle(), direction.into(), channel)
        })
    }

    pub fn has_iq_balance(&self, direction: Direction, channel: usize) -> Result<bool, Error> {
        self.check(unsafe {
            (self.entries().has_iq_balance)(self.handle(), direction.into(), channel)
        })
    }

    pub fn frequency(&self, direction: Direction, channel: usize) -> Result<f64, Error> {
        self.check(unsafe { (self.entries().frequency)(self.handle(), direction.into(), channel) })
    }

    pub fn set_frequency(
        &self,
        direction: Direction,
        channel: usize,
        frequency_hz: f64,
        args: impl Into<Args>,
    ) -> Result<(), Error> {
        let tuning = Tuning::new(&args.into())?;
        self.returned(unsafe {
            (self.entries().set_frequency)(
                self.handle(),
                direction.into(),
                channel,
                frequency_hz,
                &raw const tuning.raw,
            )
        })
    }

    pub fn set_component_frequency(
        &self,
        direction: Direction,
        channel: usize,
        name: &str,
        frequency_hz: f64,
        args: impl Into<Args>,
    ) -> Result<(), Error> {
        let name = cstring(name)?;
        let tuning = Tuning::new(&args.into())?;
        self.returned(unsafe {
            (self.entries().set_frequency_component)(
                self.handle(),
                direction.into(),
                channel,
                name.as_ptr(),
                frequency_hz,
                &raw const tuning.raw,
            )
        })
    }

    pub fn list_frequencies(
        &self,
        direction: Direction,
        channel: usize,
    ) -> Result<Vec<String>, Error> {
        self.strings(|length| unsafe {
            (self.entries().list_frequencies)(self.handle(), direction.into(), channel, length)
        })
    }

    pub fn frequency_range(
        &self,
        direction: Direction,
        channel: usize,
    ) -> Result<Vec<Range>, Error> {
        self.ranges(|length| unsafe {
            (self.entries().frequency_range)(self.handle(), direction.into(), channel, length)
        })
    }

    pub fn sample_rate(&self, direction: Direction, channel: usize) -> Result<f64, Error> {
        self.check(unsafe {
            (self.entries().sample_rate)(self.handle(), direction.into(), channel)
        })
    }

    pub fn set_sample_rate(
        &self,
        direction: Direction,
        channel: usize,
        rate: f64,
    ) -> Result<(), Error> {
        self.returned(unsafe {
            (self.entries().set_sample_rate)(self.handle(), direction.into(), channel, rate)
        })
    }

    pub fn get_sample_rate_range(
        &self,
        direction: Direction,
        channel: usize,
    ) -> Result<Vec<Range>, Error> {
        self.ranges(|length| unsafe {
            (self.entries().sample_rate_range)(self.handle(), direction.into(), channel, length)
        })
    }

    pub fn bandwidth(&self, direction: Direction, channel: usize) -> Result<f64, Error> {
        self.check(unsafe { (self.entries().bandwidth)(self.handle(), direction.into(), channel) })
    }

    pub fn set_bandwidth(
        &self,
        direction: Direction,
        channel: usize,
        bandwidth_hz: f64,
    ) -> Result<(), Error> {
        self.returned(unsafe {
            (self.entries().set_bandwidth)(self.handle(), direction.into(), channel, bandwidth_hz)
        })
    }

    pub fn bandwidth_range(
        &self,
        direction: Direction,
        channel: usize,
    ) -> Result<Vec<Range>, Error> {
        self.ranges(|length| unsafe {
            (self.entries().bandwidth_range)(self.handle(), direction.into(), channel, length)
        })
    }

    pub fn list_clock_sources(&self) -> Result<Vec<String>, Error> {
        self.strings(|length| unsafe { (self.entries().list_clock_sources)(self.handle(), length) })
    }

    pub fn get_clock_source(&self) -> Result<String, Error> {
        self.string(unsafe { (self.entries().clock_source)(self.handle()) })
    }

    pub fn list_time_sources(&self) -> Result<Vec<String>, Error> {
        self.strings(|length| unsafe { (self.entries().list_time_sources)(self.handle(), length) })
    }

    pub fn get_time_source(&self) -> Result<String, Error> {
        self.string(unsafe { (self.entries().time_source)(self.handle()) })
    }

    pub fn has_hardware_time(&self, what: Option<&str>) -> Result<bool, Error> {
        let what = cstring(what.unwrap_or_default())?;
        self.check(unsafe { (self.entries().has_hardware_time)(self.handle(), what.as_ptr()) })
    }

    pub fn get_hardware_time(&self, what: Option<&str>) -> Result<i64, Error> {
        let what = cstring(what.unwrap_or_default())?;
        self.check(unsafe { (self.entries().hardware_time)(self.handle(), what.as_ptr()) })
    }

    pub fn get_master_clock_rate(&self) -> Result<f64, Error> {
        self.check(unsafe { (self.entries().master_clock_rate)(self.handle()) })
    }

    pub fn read_setting(&self, key: &str) -> Result<String, Error> {
        let key = cstring(key)?;
        self.string(unsafe { (self.entries().read_setting)(self.handle(), key.as_ptr()) })
    }

    pub fn write_setting(&self, key: &str, value: &str) -> Result<(), Error> {
        let key = cstring(key)?;
        let value = cstring(value)?;
        self.returned(unsafe {
            (self.entries().write_setting)(self.handle(), key.as_ptr(), value.as_ptr())
        })
    }

    pub fn rx_stream<S: StreamSample>(&self, channels: &[usize]) -> Result<RxStream<S>, Error> {
        RxStream::open(self, channels)
    }

    pub fn tx_stream<S: StreamSample>(&self, channels: &[usize]) -> Result<TxStream<S>, Error> {
        TxStream::open(self, channels)
    }
}

fn last_error(library: &Library) -> String {
    let pointer = unsafe { (library.entries.last_error)() };
    if pointer.is_null() {
        return "the SoapySDR call failed without a message".to_string();
    }
    unsafe { std::ffi::CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn cstring(text: &str) -> Result<CString, Error> {
    CString::new(text).map_err(|_| Error::unsupported(format!("{text:?} contains a null byte")))
}

/// Tuning arguments held alive for the duration of one call: SoapySDR reads the key and value
/// pointers out of the struct, so the strings they point at must outlive it.
struct Tuning {
    raw: ffi::Kwargs,
    _keys: Vec<CString>,
    _values: Vec<CString>,
    _key_pointers: Vec<*mut c_char>,
    _value_pointers: Vec<*mut c_char>,
}

impl Tuning {
    fn new(args: &Args) -> Result<Self, Error> {
        let keys: Vec<CString> = args
            .iter()
            .map(|(key, _)| cstring(key))
            .collect::<Result<_, _>>()?;
        let values: Vec<CString> = args
            .iter()
            .map(|(_, value)| cstring(value))
            .collect::<Result<_, _>>()?;
        let mut key_pointers: Vec<*mut c_char> =
            keys.iter().map(|key| key.as_ptr().cast_mut()).collect();
        let mut value_pointers: Vec<*mut c_char> = values
            .iter()
            .map(|value| value.as_ptr().cast_mut())
            .collect();
        let raw = ffi::Kwargs {
            size: key_pointers.len(),
            keys: key_pointers.as_mut_ptr(),
            vals: value_pointers.as_mut_ptr(),
        };
        Ok(Self {
            raw,
            _keys: keys,
            _values: values,
            _key_pointers: key_pointers,
            _value_pointers: value_pointers,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn a_vendor_list_is_freed_on_every_path_out() {
        let freed = Cell::new(None);
        let mut value = 7u32;
        {
            let _owned = Owned::new(&raw mut value, 3, |pointer, length| {
                freed.set(Some((unsafe { *pointer }, length)));
            });
        }
        assert_eq!(freed.get(), Some((7, 3)));
    }

    #[test]
    fn a_list_the_driver_never_allocated_is_not_freed() {
        let freed = Cell::new(false);
        {
            let _owned = Owned::new(std::ptr::null_mut::<u32>(), 0, |_, _| freed.set(true));
        }
        assert!(!freed.get());
    }
}

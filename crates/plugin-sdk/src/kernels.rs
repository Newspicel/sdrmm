//! The heavy `dsp` kernels, run natively by the host when the plugin runs as WASM.
//!
//! Built for any other target, for the plugin's own tests, they run `sdrmm_dsp` in-process, so a
//! plugin behaves the same under `cargo test` as inside the server.

use num_complex::Complex;

#[cfg(target_arch = "wasm32")]
mod host {
    #[link(wasm_import_module = "sdrmm")]
    unsafe extern "C" {
        pub fn fir_new(taps_ptr: u32, taps_len: u32, decimation: u32) -> i32;
        pub fn fir_process(
            handle: i32,
            in_ptr: u32,
            in_len: u32,
            out_ptr: u32,
            out_cap: u32,
        ) -> i32;
        pub fn fft_new(len: u32, inverse: u32) -> i32;
        pub fn fft_run(handle: i32, buf_ptr: u32, len: u32) -> i32;
        pub fn resampler_new(ratio: f64) -> i32;
        pub fn resampler_process(
            handle: i32,
            in_ptr: u32,
            in_len: u32,
            out_ptr: u32,
            out_cap: u32,
        ) -> i32;
        pub fn kernel_drop(handle: i32);
        pub fn log(level: u32, ptr: u32, len: u32);
    }

    pub fn address<T>(slice: &[T]) -> u32 {
        slice.as_ptr() as usize as u32
    }

    pub fn len(len: usize) -> u32 {
        u32::try_from(len).unwrap_or(u32::MAX)
    }

    pub fn checked(handle: i32, what: &str) -> Result<i32, String> {
        if handle < 0 {
            Err(format!("host refused the {what} kernel"))
        } else {
            Ok(handle)
        }
    }
}

/// A complex FIR filter with decimation.
pub struct Fir {
    #[cfg(target_arch = "wasm32")]
    handle: i32,
    #[cfg(target_arch = "wasm32")]
    decimation: usize,
    #[cfg(not(target_arch = "wasm32"))]
    inner: sdrmm_dsp::Decimator,
}

impl Fir {
    pub fn new(taps: &[f32], decimation: usize) -> Result<Self, String> {
        if taps.is_empty() || decimation == 0 {
            return Err("a FIR needs taps and a decimation of at least 1".to_owned());
        }
        #[cfg(target_arch = "wasm32")]
        {
            // SAFETY: the host reads `taps` from this module's memory during the call only.
            let handle = unsafe {
                host::fir_new(
                    host::address(taps),
                    host::len(taps.len()),
                    host::len(decimation),
                )
            };
            Ok(Self {
                handle: host::checked(handle, "FIR")?,
                decimation,
            })
        }
        #[cfg(not(target_arch = "wasm32"))]
        Ok(Self {
            inner: sdrmm_dsp::Decimator::new(taps, decimation),
        })
    }

    /// Filters `input` into `out`, replacing what `out` held.
    pub fn process(&mut self, input: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        #[cfg(target_arch = "wasm32")]
        {
            out.resize(input.len() / self.decimation + 1, Complex::new(0.0, 0.0));
            // SAFETY: both buffers live in this module's memory for the whole call.
            let written = unsafe {
                host::fir_process(
                    self.handle,
                    host::address(input),
                    host::len(input.len()),
                    host::address(out),
                    host::len(out.len()),
                )
            };
            out.truncate(usize::try_from(written).unwrap_or(0));
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.inner.process(input, out);
    }
}

/// An in-place FFT of one fixed length.
pub struct Fft {
    len: usize,
    #[cfg(target_arch = "wasm32")]
    handle: i32,
    #[cfg(not(target_arch = "wasm32"))]
    inner: sdrmm_dsp::fft::Transform,
}

impl Fft {
    pub fn forward(len: usize) -> Result<Self, String> {
        Self::new(len, false)
    }

    pub fn inverse(len: usize) -> Result<Self, String> {
        Self::new(len, true)
    }

    fn new(len: usize, inverse: bool) -> Result<Self, String> {
        if len == 0 {
            return Err("an FFT needs a length".to_owned());
        }
        #[cfg(target_arch = "wasm32")]
        {
            // SAFETY: plain values cross the boundary.
            let handle = unsafe { host::fft_new(host::len(len), u32::from(inverse)) };
            Ok(Self {
                len,
                handle: host::checked(handle, "FFT")?,
            })
        }
        #[cfg(not(target_arch = "wasm32"))]
        Ok(Self {
            len,
            inner: if inverse {
                sdrmm_dsp::fft::Transform::inverse(len)
            } else {
                sdrmm_dsp::fft::Transform::forward(len)
            },
        })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Transforms the first `len` samples of `buf` in place; a shorter buffer is left alone.
    pub fn run(&mut self, buf: &mut [Complex<f32>]) {
        if buf.len() < self.len {
            return;
        }
        let buf = &mut buf[..self.len];
        #[cfg(target_arch = "wasm32")]
        // SAFETY: `buf` lives in this module's memory for the whole call.
        unsafe {
            host::fft_run(self.handle, host::address(buf), host::len(buf.len()));
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.inner.process(buf);
    }
}

/// A fractional resampler by a fixed ratio of output to input rate.
pub struct Resampler {
    #[cfg(target_arch = "wasm32")]
    handle: i32,
    #[cfg(target_arch = "wasm32")]
    ratio: f64,
    #[cfg(not(target_arch = "wasm32"))]
    inner: sdrmm_dsp::FracResampler,
}

impl Resampler {
    pub fn new(ratio: f64) -> Result<Self, String> {
        if !(ratio.is_finite() && ratio > 0.0) {
            return Err(format!("a resampling ratio must be positive, got {ratio}"));
        }
        #[cfg(target_arch = "wasm32")]
        {
            // SAFETY: a plain value crosses the boundary.
            let handle = unsafe { host::resampler_new(ratio) };
            Ok(Self {
                handle: host::checked(handle, "resampler")?,
                ratio,
            })
        }
        #[cfg(not(target_arch = "wasm32"))]
        Ok(Self {
            inner: sdrmm_dsp::FracResampler::new(ratio),
        })
    }

    pub fn process(&mut self, input: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        #[cfg(target_arch = "wasm32")]
        {
            let cap = (input.len() as f64 * self.ratio).ceil() as usize + 16;
            out.resize(cap, Complex::new(0.0, 0.0));
            // SAFETY: both buffers live in this module's memory for the whole call.
            let written = unsafe {
                host::resampler_process(
                    self.handle,
                    host::address(input),
                    host::len(input.len()),
                    host::address(out),
                    host::len(out.len()),
                )
            };
            out.truncate(usize::try_from(written).unwrap_or(0));
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.inner.process(input, out);
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for Fir {
    fn drop(&mut self) {
        // SAFETY: a plain handle crosses the boundary.
        unsafe { host::kernel_drop(self.handle) }
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for Fft {
    fn drop(&mut self) {
        // SAFETY: a plain handle crosses the boundary.
        unsafe { host::kernel_drop(self.handle) }
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for Resampler {
    fn drop(&mut self) {
        // SAFETY: a plain handle crosses the boundary.
        unsafe { host::kernel_drop(self.handle) }
    }
}

/// Writes a line to the server log.
pub fn log(message: &str) {
    #[cfg(target_arch = "wasm32")]
    // SAFETY: the host reads `message` from this module's memory during the call only.
    unsafe {
        host::log(
            0,
            host::address(message.as_bytes()),
            host::len(message.len()),
        );
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = message;
    }
}

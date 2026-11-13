//! The native `dsp` kernels a plugin imports, working on the plugin's own memory.

use num_complex::Complex;
use sdrmm_dsp::{Decimator, FracResampler, fft::Transform};
use sdrmm_wire::plugin::{IMPORT_MODULE, import};
use wasmtime::{Caller, Linker, Memory, StoreLimits};

use crate::{
    PluginError,
    engine::limits,
    samples::{SAMPLE_BYTES, floats, read_samples, write_samples},
};

const MAX_KERNELS: usize = 64;
const MAX_TAPS: usize = 1 << 16;
const MAX_FFT: usize = 1 << 22;

enum Kernel {
    Fir(Decimator),
    Fft(Transform),
    Resampler(FracResampler),
}

/// What a plugin instance holds on the host side.
pub(crate) struct HostState {
    kernels: Vec<Option<Kernel>>,
    input: Vec<Complex<f32>>,
    output: Vec<Complex<f32>>,
    pub(crate) plugin: String,
    pub(crate) limits: StoreLimits,
}

impl HostState {
    pub(crate) fn new(plugin: &str) -> Self {
        Self {
            kernels: Vec::new(),
            input: Vec::new(),
            output: Vec::new(),
            plugin: plugin.to_owned(),
            limits: limits(),
        }
    }

    fn add(&mut self, kernel: Kernel) -> i32 {
        let slot = self.kernels.iter().position(Option::is_none);
        let index = match slot {
            Some(free) => free,
            None if self.kernels.len() < MAX_KERNELS => {
                self.kernels.push(None);
                self.kernels.len() - 1
            }
            None => return -1,
        };
        self.kernels[index] = Some(kernel);
        i32::try_from(index).unwrap_or(-1)
    }

    fn kernel(&mut self, handle: i32) -> Option<&mut Kernel> {
        let index = usize::try_from(handle).ok()?;
        self.kernels.get_mut(index)?.as_mut()
    }
}

fn memory(caller: &mut Caller<'_, HostState>) -> Option<Memory> {
    caller.get_export("memory")?.into_memory()
}

fn span(ptr: u32, len: u32, unit: usize, total: usize) -> Option<std::ops::Range<usize>> {
    let start = usize::try_from(ptr).ok()?;
    let bytes = usize::try_from(len).ok()?.checked_mul(unit)?;
    let end = start.checked_add(bytes)?;
    (end <= total).then_some(start..end)
}

/// Runs `stage` on the samples at `in_ptr` and writes up to `out_cap` results to `out_ptr`;
/// returns how many were written, or -1 for a bad handle or a range outside the memory.
fn stream(
    caller: &mut Caller<'_, HostState>,
    handle: i32,
    (in_ptr, in_len): (u32, u32),
    (out_ptr, out_cap): (u32, u32),
) -> i32 {
    let Some(memory) = memory(caller) else {
        return -1;
    };
    let (data, state) = memory.data_and_store_mut(caller);
    let total = data.len();
    let (Some(input), Some(output)) = (
        span(in_ptr, in_len, SAMPLE_BYTES, total),
        span(out_ptr, out_cap, SAMPLE_BYTES, total),
    ) else {
        return -1;
    };
    let HostState {
        kernels,
        input: scratch_in,
        output: scratch_out,
        ..
    } = state;
    read_samples(&data[input], scratch_in);
    let Some(kernel) = usize::try_from(handle)
        .ok()
        .and_then(|index| kernels.get_mut(index))
        .and_then(Option::as_mut)
    else {
        return -1;
    };
    match kernel {
        Kernel::Fir(fir) => fir.process(scratch_in, scratch_out),
        Kernel::Resampler(resampler) => resampler.process(scratch_in, scratch_out),
        Kernel::Fft(_) => return -1,
    }
    let written = scratch_out.len().min(out_cap as usize);
    write_samples(&scratch_out[..written], &mut data[output]);
    i32::try_from(written).unwrap_or(-1)
}

fn fir_new(
    mut caller: Caller<'_, HostState>,
    taps_ptr: u32,
    taps_len: u32,
    decimation: u32,
) -> i32 {
    let Some(memory) = memory(&mut caller) else {
        return -1;
    };
    let (data, state) = memory.data_and_store_mut(&mut caller);
    let Some(range) = span(taps_ptr, taps_len, 4, data.len()) else {
        return -1;
    };
    if taps_len == 0 || taps_len as usize > MAX_TAPS || decimation == 0 {
        return -1;
    }
    let taps: Vec<f32> = floats(&data[range]).collect();
    state.add(Kernel::Fir(Decimator::new(&taps, decimation as usize)))
}

fn fft_new(mut caller: Caller<'_, HostState>, len: u32, inverse: u32) -> i32 {
    let len = len as usize;
    if len == 0 || len > MAX_FFT {
        return -1;
    }
    let transform = if inverse == 0 {
        Transform::forward(len)
    } else {
        Transform::inverse(len)
    };
    caller.data_mut().add(Kernel::Fft(transform))
}

fn fft_run(mut caller: Caller<'_, HostState>, handle: i32, ptr: u32, len: u32) -> i32 {
    let Some(memory) = memory(&mut caller) else {
        return -1;
    };
    let (data, state) = memory.data_and_store_mut(&mut caller);
    let Some(range) = span(ptr, len, SAMPLE_BYTES, data.len()) else {
        return -1;
    };
    let HostState { kernels, input, .. } = state;
    let Some(Some(Kernel::Fft(transform))) = usize::try_from(handle)
        .ok()
        .and_then(|index| kernels.get_mut(index))
    else {
        return -1;
    };
    if transform.len() != len as usize {
        return -1;
    }
    read_samples(&data[range.clone()], input);
    transform.process(input);
    write_samples(input, &mut data[range]);
    0
}

fn resampler_new(mut caller: Caller<'_, HostState>, ratio: f64) -> i32 {
    if !(ratio.is_finite() && ratio > 0.0) {
        return -1;
    }
    caller
        .data_mut()
        .add(Kernel::Resampler(FracResampler::new(ratio)))
}

fn kernel_drop(mut caller: Caller<'_, HostState>, handle: i32) {
    let state = caller.data_mut();
    if state.kernel(handle).is_some()
        && let Ok(index) = usize::try_from(handle)
    {
        state.kernels[index] = None;
    }
}

fn log(mut caller: Caller<'_, HostState>, _level: u32, ptr: u32, len: u32) {
    let Some(memory) = memory(&mut caller) else {
        return;
    };
    let data = memory.data(&caller);
    let Some(range) = span(ptr, len, 1, data.len()) else {
        return;
    };
    let line = String::from_utf8_lossy(&data[range]);
    tracing::info!(plugin = %caller.data().plugin, "{line}");
}

pub(crate) fn link(linker: &mut Linker<HostState>) -> Result<(), PluginError> {
    let m = IMPORT_MODULE;
    linker
        .func_wrap(m, import::LOG, log)
        .and_then(|l| l.func_wrap(m, import::FIR_NEW, fir_new))
        .and_then(|l| {
            l.func_wrap(
                m,
                import::FIR_PROCESS,
                |mut c: Caller<'_, HostState>, h: i32, ip: u32, il: u32, op: u32, oc: u32| {
                    stream(&mut c, h, (ip, il), (op, oc))
                },
            )
        })
        .and_then(|l| l.func_wrap(m, import::FFT_NEW, fft_new))
        .and_then(|l| l.func_wrap(m, import::FFT_RUN, fft_run))
        .and_then(|l| l.func_wrap(m, import::RESAMPLER_NEW, resampler_new))
        .and_then(|l| {
            l.func_wrap(
                m,
                import::RESAMPLER_PROCESS,
                |mut c: Caller<'_, HostState>, h: i32, ip: u32, il: u32, op: u32, oc: u32| {
                    stream(&mut c, h, (ip, il), (op, oc))
                },
            )
        })
        .and_then(|l| l.func_wrap(m, import::KERNEL_DROP, kernel_drop))
        .map(|_| ())
        .map_err(|error| PluginError::Link(error.to_string()))
}

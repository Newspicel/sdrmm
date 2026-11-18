//! What the engine gives a processor that runs dedicated: thread priority and the kernels its
//! accelerators run. The runner itself is the processor's own.

use std::sync::{Arc, OnceLock};

use sdrmm_array::{
    ArrayCtx, Built, Params, ProcessorDescriptor, RunnerHost,
    kernel::{CafKernel, CafPlan},
};
use sdrmm_decoder::ChannelError;
use sdrmm_device::{Latency, schedule::claim};

pub(crate) use sdrmm_array::{DedicatedPlan, DedicatedRunner, Prepared};

struct EngineHost;

impl RunnerHost for EngineHost {
    fn raise(&self) {
        claim(Latency::Interactive);
    }

    fn caf(&self, plan: &CafPlan<'_>) -> Result<Box<dyn CafKernel>, String> {
        accelerated_caf(plan)
    }
}

#[cfg(feature = "gpu-fft")]
fn accelerated_caf(plan: &CafPlan<'_>) -> Result<Box<dyn CafKernel>, String> {
    let context = crate::gpu::context().map_err(str::to_owned)?;
    let kernel = crate::gpu::caf::GpuCaf::new(Arc::clone(context), plan)?;
    Ok(Box::new(kernel))
}

#[cfg(not(feature = "gpu-fft"))]
fn accelerated_caf(_plan: &CafPlan<'_>) -> Result<Box<dyn CafKernel>, String> {
    Err("this build has no GPU support".to_owned())
}

/// The host every dedicated runner the engine builds runs on.
pub(crate) fn engine_host() -> &'static Arc<dyn RunnerHost> {
    static HOST: OnceLock<Arc<dyn RunnerHost>> = OnceLock::new();
    HOST.get_or_init(|| Arc::new(EngineHost))
}

pub(crate) fn build_dedicated(
    descriptor: &dyn ProcessorDescriptor,
    ctx: &ArrayCtx<'_>,
    params: &Params,
) -> Result<Built, ChannelError> {
    descriptor.dedicated(ctx, params, engine_host())
}

#[cfg(test)]
mod tests;

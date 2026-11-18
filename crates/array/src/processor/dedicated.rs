//! Processors that run on threads and backends of their own, which only a host such as the
//! engine can give them. A processor asks for this with [`super::Execution::Dedicated`] and
//! builds its runner through [`super::ProcessorDescriptor::dedicated`].

use std::{any::Any, sync::Arc, thread::JoinHandle};

use sdrmm_decoder::ChannelError;

use super::{
    ArrayBlock, ArrayCtx, GpuUse, Params, ProcessorAction, ProcessorFaults, ProcessorOutput,
};
use crate::kernel::{CafKernel, CafPlan};

/// What a dedicated runner needs from its host and nothing else.
pub trait RunnerHost: Send + Sync {
    /// Raises the calling thread to the priority a runner's worker needs.
    fn raise(&self);

    /// A cross-ambiguity kernel on an accelerator that fits `plan`, when the host has one.
    fn caf(&self, plan: &CafPlan<'_>) -> Result<Box<dyn CafKernel>, String>;
}

/// What a settings change asks of a running dedicated runner. Weighed on the control plane, so
/// committing it on the DSP thread only swaps what was made here.
pub enum Prepared {
    Same,
    /// Settings the runner takes without a rebuild, in the runner's own type.
    Live(Box<dyn Any + Send>),
    Rebuild(Box<dyn DedicatedRunner>),
}

pub trait DedicatedRunner: Send {
    fn push(&mut self, block: &ArrayBlock<'_>);
    fn poll(&mut self, out: &mut ProcessorOutput<'_>);
    fn commit(&mut self, prepared: Prepared) -> Option<Box<dyn DedicatedRunner>>;
    fn action(&mut self, action: ProcessorAction) -> Result<(), ChannelError>;
    fn faults(&self) -> ProcessorFaults;
    fn dropped_samples(&self) -> u64;
    fn running(&self) -> bool;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn retire(self: Box<Self>) -> Retired;
}

/// What a runner was built on, kept by the host so the next settings change can be weighed
/// against it.
pub trait DedicatedPlan: Send + Sync {
    fn prepare(
        &self,
        ctx: &ArrayCtx<'_>,
        params: &Params,
        gpu: GpuUse,
        host: &Arc<dyn RunnerHost>,
    ) -> Result<(Prepared, Arc<dyn DedicatedPlan>), ChannelError>;

    fn as_any(&self) -> &dyn Any;
}

pub type Built = (Box<dyn DedicatedRunner>, Arc<dyn DedicatedPlan>);

pub type DedicatedFn =
    fn(&ArrayCtx<'_>, &Params, GpuUse, &Arc<dyn RunnerHost>) -> Result<Built, ChannelError>;

/// The threads of a retired runner, joined off the DSP thread.
pub struct Retired {
    threads: Vec<JoinHandle<()>>,
}

impl Retired {
    #[must_use]
    pub const fn new(threads: Vec<JoinHandle<()>>) -> Self {
        Self { threads }
    }

    pub fn join(self) -> Result<(), String> {
        let panicked = self
            .threads
            .into_iter()
            .map(JoinHandle::join)
            .filter(Result::is_err)
            .count();
        if panicked == 0 {
            Ok(())
        } else {
            Err(format!("{panicked} runner threads panicked"))
        }
    }
}

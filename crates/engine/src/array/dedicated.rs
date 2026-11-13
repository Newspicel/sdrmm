//! Processors that run on threads and backends of their own, which only the engine can give
//! them, such as a GPU. A processor asks for this with [`Execution::Dedicated`]; the runner that
//! serves it registers here beside the others.

use std::{any::Any, sync::Arc, thread::JoinHandle};

use sdrmm_channels::{
    ChannelError,
    array_processor::{
        ArrayBlock, ArrayCtx, ProcessorAction, ProcessorDescriptor, ProcessorFaults,
        ProcessorOutput,
    },
};
use sdrmm_wire::{GpuUse, ProcessorParams};

use super::radar;

/// What a settings change asks of a running dedicated runner. Weighed on the control plane, so
/// committing it on the DSP thread only swaps what was made here.
pub(crate) enum Prepared {
    Same,
    /// Settings the runner takes without a rebuild, in the runner's own type.
    Live(Box<dyn Any + Send>),
    Rebuild(Box<dyn DedicatedRunner>),
}

pub(crate) trait DedicatedRunner: Send {
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

/// What a runner was built on, kept by the engine so the next settings change can be weighed
/// against it.
pub(crate) trait DedicatedPlan: Send + Sync {
    fn prepare(
        &self,
        ctx: &ArrayCtx<'_>,
        params: &ProcessorParams,
        gpu: GpuUse,
    ) -> Result<(Prepared, Arc<dyn DedicatedPlan>), ChannelError>;

    #[cfg(test)]
    fn as_any(&self) -> &dyn Any;
}

pub(crate) type Built = (Box<dyn DedicatedRunner>, Arc<dyn DedicatedPlan>);

/// The runner of one processor that runs dedicated.
pub(crate) struct Dedicated {
    pub(crate) descriptor: &'static ProcessorDescriptor,
    pub(crate) build: fn(&ArrayCtx<'_>, &ProcessorParams, GpuUse) -> Result<Built, ChannelError>,
}

static RUNNERS: &[&Dedicated] = &[&radar::DEDICATED];

pub(crate) fn build_dedicated(
    descriptor: &ProcessorDescriptor,
    ctx: &ArrayCtx<'_>,
    params: &ProcessorParams,
) -> Result<Built, ChannelError> {
    let runner = RUNNERS
        .iter()
        .find(|runner| runner.descriptor.type_id == descriptor.type_id)
        .ok_or_else(|| ChannelError::Unsupported(format!("{} has no runner", descriptor.name)))?;
    (runner.build)(ctx, params, (descriptor.gpu)(params))
}

pub(crate) struct Retired {
    threads: Vec<JoinHandle<()>>,
}

impl Retired {
    pub(crate) const fn new(threads: Vec<JoinHandle<()>>) -> Self {
        Self { threads }
    }

    pub(crate) fn join(self) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use sdrmm_channels::array_processor::registrations;
    use sdrmm_wire::DfParams;

    use super::*;
    use crate::array::host::tests::{TALKER, frame, plan, taps};

    #[test]
    fn every_runner_serves_a_registered_processor() {
        for runner in RUNNERS {
            assert!(
                registrations()
                    .iter()
                    .any(|entry| std::ptr::eq(entry.descriptor, runner.descriptor)),
                "{} is not registered",
                runner.descriptor.type_id
            );
        }
    }

    #[test]
    fn a_processor_without_a_runner_is_refused() {
        let taps = taps(0);
        let plan = plan(
            "talker",
            ProcessorParams::Df(DfParams::default()),
            &taps.sinks,
            2,
        );
        let frame = frame(2);
        let ctx = plan.shape.ctx(&plan.node, &frame);
        let Err(ChannelError::Unsupported(text)) = build_dedicated(&TALKER, &ctx, &plan.params)
        else {
            panic!("a processor with no runner is refused");
        };
        assert_eq!(text, "Talker has no runner");
    }
}

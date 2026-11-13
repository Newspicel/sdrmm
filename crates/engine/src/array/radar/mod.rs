mod crew;
mod gpu;
mod publish;
#[cfg(test)]
mod tests;
mod worker;

use std::{num::NonZeroUsize, sync::Arc};

use sdrmm_channels::{
    ChannelError,
    array_processor::ArrayCtx,
    passive_radar::{
        CafBackend, CpuCaf, PlanChange, REGISTRATION, RadarPlan as StagePlan, change, plan_for,
    },
};
use sdrmm_wire::{GpuUse, ProcessorParams};

use super::dedicated::{Dedicated, DedicatedPlan, Prepared};

pub(crate) use worker::RadarWorker;
use worker::Shared;

const MAX_CREW: usize = 3;
const RESERVED_CORES: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CafBackendKind {
    Cpu,
    Crew { threads: u32 },
    Gpu,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RadarPlan {
    pub(crate) stage: StagePlan,
    pub(crate) backend: CafBackendKind,
}

#[cfg(test)]
impl RadarPlan {
    pub(crate) const fn ctx(&self) -> &sdrmm_channels::passive_radar::RadarCtx {
        &self.stage.ctx
    }

    pub(crate) const fn params(&self) -> &sdrmm_wire::PassiveRadarParams {
        &self.stage.params
    }
}

pub(crate) static DEDICATED: Dedicated = Dedicated {
    descriptor: REGISTRATION.descriptor,
    build: |ctx, params, gpu| {
        let (worker, plan) = build(ctx, params, gpu)?;
        Ok((Box::new(worker), plan))
    },
};

impl DedicatedPlan for RadarPlan {
    fn prepare(
        &self,
        ctx: &ArrayCtx<'_>,
        params: &ProcessorParams,
        gpu: GpuUse,
    ) -> Result<(Prepared, Arc<dyn DedicatedPlan>), ChannelError> {
        let (prepared, plan) = prepare(self, ctx, params, gpu)?;
        Ok((prepared, plan))
    }

    #[cfg(test)]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub(crate) fn build(
    ctx: &ArrayCtx<'_>,
    params: &ProcessorParams,
    gpu: GpuUse,
) -> Result<(RadarWorker, Arc<RadarPlan>), ChannelError> {
    let stage = plan_for(ctx, params)?;
    let (worker, backend) = launch(&stage, ctx.max_block, gpu)?;
    Ok((worker, Arc::new(RadarPlan { stage, backend })))
}

pub(crate) fn prepare(
    plan: &RadarPlan,
    ctx: &ArrayCtx<'_>,
    params: &ProcessorParams,
    gpu: GpuUse,
) -> Result<(Prepared, Arc<RadarPlan>), ChannelError> {
    let stage = plan_for(ctx, params)?;
    let gpu_withdrawn = plan.backend == CafBackendKind::Gpu && gpu == GpuUse::Off;
    let kept = |stage: StagePlan| {
        Arc::new(RadarPlan {
            stage,
            backend: plan.backend,
        })
    };
    match change(&plan.stage, &stage) {
        PlanChange::Same if !gpu_withdrawn => Ok((Prepared::Same, kept(stage))),
        PlanChange::Live if !gpu_withdrawn => {
            let live = stage.live();
            if !live.cfar.valid() {
                return Err(ChannelError::Refused("Guard out of range"));
            }
            if !live.tracker.valid() {
                return Err(ChannelError::Refused("Gate out of range"));
            }
            Ok((Prepared::Live(Box::new(live)), kept(stage)))
        }
        _ => {
            let (worker, backend) = launch(&stage, ctx.max_block, gpu)?;
            Ok((
                Prepared::Rebuild(Box::new(worker)),
                Arc::new(RadarPlan { stage, backend }),
            ))
        }
    }
}

fn launch(
    stage: &StagePlan,
    block: usize,
    gpu: GpuUse,
) -> Result<(RadarWorker, CafBackendKind), ChannelError> {
    let shared = Arc::new(Shared::default());
    let (backend, kind) = select_backend(stage, gpu, &shared)?;
    let worker = RadarWorker::start(stage, backend, block, shared)?;
    Ok((worker, kind))
}

fn select_backend(
    stage: &StagePlan,
    gpu: GpuUse,
    shared: &Arc<Shared>,
) -> Result<(Box<dyn CafBackend>, CafBackendKind), ChannelError> {
    let (cpu, kind) = cpu_backend(stage, shared)?;
    if gpu != GpuUse::Auto || stage.params.gpu != GpuUse::Auto {
        return Ok((cpu, kind));
    }
    Ok(match gpu::offer(stage, cpu, shared) {
        #[cfg(feature = "gpu-fft")]
        gpu::Offer::Gpu(backend) => (backend, CafBackendKind::Gpu),
        gpu::Offer::Cpu(cpu) => (cpu, kind),
    })
}

fn cpu_backend(
    stage: &StagePlan,
    shared: &Arc<Shared>,
) -> Result<(Box<dyn CafBackend>, CafBackendKind), ChannelError> {
    let helpers = crew_helpers();
    if helpers > 0 {
        match crew::CrewCaf::new(stage, helpers, shared) {
            Ok(crew) => {
                let threads = crew.threads();
                return Ok((Box::new(crew), CafBackendKind::Crew { threads }));
            }
            Err(error) => {
                tracing::warn!(%error, "radar crew did not start, CAF runs on one thread")
            }
        }
    }
    Ok((Box::new(CpuCaf::new(stage)?), CafBackendKind::Cpu))
}

fn crew_helpers() -> usize {
    std::thread::available_parallelism()
        .map_or(1, NonZeroUsize::get)
        .saturating_sub(RESERVED_CORES)
        .min(MAX_CREW)
}

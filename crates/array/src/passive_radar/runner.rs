//! The passive radar's own threads: CPI assembly, the CAF crew and an accelerator when the
//! host offers one faster than the CPU.

mod accelerated;
mod crew;
mod publish;
#[cfg(test)]
mod tests;
mod worker;

use std::{num::NonZeroUsize, sync::Arc};

use sdrmm_decoder::ChannelError;

use super::{CafBackend, CpuCaf, PlanChange, RadarPlan as StagePlan, change, plan_for};
use crate::{ArrayCtx, Built, DedicatedPlan, GpuUse, Params, Prepared, RunnerHost};

pub use worker::RadarWorker;
use worker::Shared;

const MAX_CREW: usize = 3;
const RESERVED_CORES: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CafBackendKind {
    Cpu,
    Crew { threads: u32 },
    Gpu,
}

/// What a radar runner was built on: its stage plan and the backend its CAF runs on.
#[derive(Clone, Debug, PartialEq)]
pub struct RunnerPlan {
    pub stage: StagePlan,
    pub backend: CafBackendKind,
}

impl DedicatedPlan for RunnerPlan {
    fn prepare(
        &self,
        ctx: &ArrayCtx<'_>,
        params: &Params,
        gpu: GpuUse,
        host: &Arc<dyn RunnerHost>,
    ) -> Result<(Prepared, Arc<dyn DedicatedPlan>), ChannelError> {
        let (prepared, plan) = prepare(self, ctx, params, gpu, host)?;
        Ok((prepared, plan))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The radar's runner, on threads and kernels `host` gives.
pub fn dedicated(
    ctx: &ArrayCtx<'_>,
    params: &Params,
    gpu: GpuUse,
    host: &Arc<dyn RunnerHost>,
) -> Result<Built, ChannelError> {
    let (worker, plan) = build(ctx, params, gpu, host)?;
    Ok((Box::new(worker), plan))
}

pub fn build(
    ctx: &ArrayCtx<'_>,
    params: &Params,
    gpu: GpuUse,
    host: &Arc<dyn RunnerHost>,
) -> Result<(RadarWorker, Arc<RunnerPlan>), ChannelError> {
    let stage = plan_for(ctx, params)?;
    let (worker, backend) = launch(&stage, ctx.max_block, gpu, host)?;
    Ok((worker, Arc::new(RunnerPlan { stage, backend })))
}

pub fn prepare(
    plan: &RunnerPlan,
    ctx: &ArrayCtx<'_>,
    params: &Params,
    gpu: GpuUse,
    host: &Arc<dyn RunnerHost>,
) -> Result<(Prepared, Arc<RunnerPlan>), ChannelError> {
    let stage = plan_for(ctx, params)?;
    let gpu_withdrawn = plan.backend == CafBackendKind::Gpu && gpu == GpuUse::Off;
    let kept = |stage: StagePlan| {
        Arc::new(RunnerPlan {
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
            let (worker, backend) = launch(&stage, ctx.max_block, gpu, host)?;
            Ok((
                Prepared::Rebuild(Box::new(worker)),
                Arc::new(RunnerPlan { stage, backend }),
            ))
        }
    }
}

fn launch(
    stage: &StagePlan,
    block: usize,
    gpu: GpuUse,
    host: &Arc<dyn RunnerHost>,
) -> Result<(RadarWorker, CafBackendKind), ChannelError> {
    let shared = Arc::new(Shared {
        host: Some(Arc::clone(host)),
        ..Shared::default()
    });
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
    Ok(match accelerated::offer(stage, cpu, shared) {
        accelerated::Offer::Accelerated(backend) => (backend, CafBackendKind::Gpu),
        accelerated::Offer::Cpu(cpu) => (cpu, kind),
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
                tracing::warn!(%error, "radar crew did not start, CAF runs on one thread");
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

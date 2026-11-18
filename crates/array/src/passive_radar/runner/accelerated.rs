//! The CAF on the host's accelerator, kept only when it races the CPU and wins, and handed back
//! to the CPU for good the first time it fails.

use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use super::{cpu_backend, worker::Shared};
use crate::kernel::{CafBackend, CafError, CafKernel, CpiJob, CubeOut};
use crate::passive_radar::RadarPlan as StagePlan;

const FASTER_BY: f64 = 1.3;
const RACE_RUNS: usize = 2;

pub(super) enum Offer {
    Accelerated(Box<dyn CafBackend>),
    Cpu(Box<dyn CafBackend>),
}

pub(super) fn keeps_accelerator(accelerated: Duration, cpu: Duration) -> bool {
    accelerated.as_secs_f64() * FASTER_BY <= cpu.as_secs_f64()
}

pub(super) fn offer(stage: &StagePlan, cpu: Box<dyn CafBackend>, shared: &Arc<Shared>) -> Offer {
    match Accelerated::new(stage, cpu, shared) {
        Ok(mut backend) => {
            let race = backend.race(stage);
            decide(backend, race)
        }
        Err((cpu, reason)) => {
            tracing::info!(%reason, "radar CAF stays on the CPU");
            Offer::Cpu(cpu)
        }
    }
}

pub(super) fn decide(backend: Accelerated, race: Result<(Duration, Duration), String>) -> Offer {
    match race {
        Ok((accelerated, cpu)) if keeps_accelerator(accelerated, cpu) => {
            tracing::info!(
                accelerated_ms = accelerated.as_secs_f64() * 1e3,
                cpu_ms = cpu.as_secs_f64() * 1e3,
                "radar CAF runs on the accelerator"
            );
            Offer::Accelerated(Box::new(backend))
        }
        Ok((accelerated, cpu)) => {
            tracing::info!(
                accelerated_ms = accelerated.as_secs_f64() * 1e3,
                cpu_ms = cpu.as_secs_f64() * 1e3,
                "radar CAF stays on the CPU, the accelerator is not faster"
            );
            Offer::Cpu(backend.cpu)
        }
        Err(reason) => {
            tracing::warn!(%reason, "radar accelerator check failed, the CAF stays on the CPU");
            Offer::Cpu(backend.cpu)
        }
    }
}

pub(super) struct Accelerated {
    kernel: Option<Box<dyn CafKernel>>,
    cpu: Box<dyn CafBackend>,
    shared: Arc<Shared>,
}

impl Accelerated {
    pub(super) fn new(
        stage: &StagePlan,
        cpu: Box<dyn CafBackend>,
        shared: &Arc<Shared>,
    ) -> Result<Self, (Box<dyn CafBackend>, String)> {
        let Some(host) = &shared.host else {
            return Err((cpu, "no runner host".to_owned()));
        };
        match host.caf(&stage.caf()) {
            Ok(kernel) => Ok(Self {
                kernel: Some(kernel),
                cpu,
                shared: Arc::clone(shared),
            }),
            Err(reason) => Err((cpu, reason)),
        }
    }

    fn race(&mut self, stage: &StagePlan) -> Result<(Duration, Duration), String> {
        let job = Arc::new(CpiJob::new(stage.shape.lanes + 1, stage.shape.window()));
        let mut out = CubeOut::new(&stage.shape);
        let kernel = self
            .kernel
            .as_mut()
            .ok_or_else(|| "no accelerator".to_owned())?;
        let accelerated = fastest(|| kernel.run(&job, &mut out))?;
        let (mut rival, _) =
            cpu_backend(stage, &Arc::new(Shared::default())).map_err(|error| error.to_string())?;
        let on_cpu = fastest(|| {
            rival
                .run_shared(&job, &mut out)
                .map_err(|error| error.to_string())
        })?;
        Ok((accelerated, on_cpu))
    }

    fn on_accelerator(&mut self, job: &CpiJob, out: &mut CubeOut) -> bool {
        let Some(kernel) = self.kernel.as_mut() else {
            return false;
        };
        match kernel.run(job, out) {
            Ok(()) => true,
            Err(error) => {
                self.kernel = None;
                self.shared.gpu_failures.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(%error, "radar accelerator failed, the CAF moves to the CPU");
                false
            }
        }
    }
}

impl CafBackend for Accelerated {
    fn run(&mut self, job: &CpiJob, out: &mut CubeOut) -> Result<(), CafError> {
        if self.on_accelerator(job, out) {
            return Ok(());
        }
        self.cpu.run(job, out)
    }

    fn run_shared(&mut self, job: &Arc<CpiJob>, out: &mut CubeOut) -> Result<(), CafError> {
        if self.on_accelerator(job, out) {
            return Ok(());
        }
        self.cpu.run_shared(job, out)
    }

    fn gpu(&self) -> bool {
        self.kernel.is_some()
    }

    fn threads(&self) -> u32 {
        if self.kernel.is_some() {
            0
        } else {
            self.cpu.threads()
        }
    }
}

fn fastest(mut run: impl FnMut() -> Result<(), String>) -> Result<Duration, String> {
    let mut best = Duration::MAX;
    for _ in 0..RACE_RUNS {
        let started = Instant::now();
        run()?;
        best = best.min(started.elapsed());
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::passive_radar::{
        ClutterParams, CpuCaf, Illuminator, PassiveRadarParams, RadarCtx, ReferenceCleaning, plan,
    };
    use crate::{GpuUse, RunnerHost, kernel::CafPlan};

    /// A kernel that runs the CPU CAF until it is told to fail.
    struct Flaky {
        cpu: CpuCaf,
        broken: Arc<AtomicBool>,
    }

    impl CafKernel for Flaky {
        fn run(&mut self, job: &CpiJob, out: &mut CubeOut) -> Result<(), String> {
            if self.broken.load(Ordering::Acquire) {
                return Err("lost the device".to_owned());
            }
            self.cpu.run(job, out).map_err(|error| error.to_string())
        }
    }

    struct FlakyHost {
        stage: StagePlan,
        broken: Arc<AtomicBool>,
    }

    impl RunnerHost for FlakyHost {
        fn raise(&self) {}

        fn caf(&self, _: &CafPlan<'_>) -> Result<Box<dyn CafKernel>, String> {
            let cpu = CpuCaf::new(&self.stage).map_err(|error| error.to_string())?;
            Ok(Box::new(Flaky {
                cpu,
                broken: Arc::clone(&self.broken),
            }))
        }
    }

    fn stage() -> StagePlan {
        let params = PassiveRadarParams {
            illuminator: Illuminator::Fm,
            clutter: ClutterParams::default(),
            aoa: false,
            reference: ReferenceCleaning::Off,
            gpu: GpuUse::Auto,
            ..PassiveRadarParams::default()
        };
        let ctx = RadarCtx {
            sample_rate: 2_400_000.0,
            center_hz: 100e6,
            elements: 3,
            positions_m: Vec::new(),
            manifold: None,
            tuned_together: true,
        };
        plan(&ctx, &params).expect("plan")
    }

    fn hosted(stage: &StagePlan) -> (Arc<Shared>, Arc<AtomicBool>) {
        let broken = Arc::new(AtomicBool::new(false));
        let host: Arc<dyn RunnerHost> = Arc::new(FlakyHost {
            stage: stage.clone(),
            broken: Arc::clone(&broken),
        });
        let shared = Arc::new(Shared {
            host: Some(host),
            ..Shared::default()
        });
        (shared, broken)
    }

    fn cpu(stage: &StagePlan) -> Box<dyn CafBackend> {
        Box::new(CpuCaf::new(stage).expect("cpu"))
    }

    #[test]
    fn keeps_the_accelerator_only_when_1_3x_faster() {
        let ms = Duration::from_millis;
        assert!(keeps_accelerator(ms(10), ms(14)));
        assert!(keeps_accelerator(ms(30), ms(120)));
        assert!(!keeps_accelerator(ms(10), ms(12)));
        assert!(!keeps_accelerator(ms(20), ms(10)));
    }

    #[test]
    fn auto_keeps_the_accelerator_only_when_it_wins() {
        let stage = stage();
        let (shared, _) = hosted(&stage);
        let ms = Duration::from_millis;
        let races = [
            (Ok((ms(10), ms(20))), true),
            (Ok((ms(10), ms(12))), false),
            (Err("no answer".to_owned()), false),
        ];
        for (race, wins) in races {
            let backend = Accelerated::new(&stage, cpu(&stage), &shared)
                .map_err(|(_, reason)| reason)
                .expect("an accelerator");
            match decide(backend, race) {
                Offer::Accelerated(kept) => assert!(wins && kept.gpu()),
                Offer::Cpu(kept) => assert!(!wins && !kept.gpu()),
            }
        }
    }

    #[test]
    fn an_accelerator_error_recomputes_on_the_cpu_for_good() {
        let stage = stage();
        let (shared, broken) = hosted(&stage);
        let mut backend = Accelerated::new(&stage, cpu(&stage), &shared)
            .map_err(|(_, reason)| reason)
            .expect("an accelerator");
        let job = Arc::new(CpiJob::new(stage.shape.lanes + 1, stage.shape.window()));
        let mut want = CubeOut::new(&stage.shape);
        cpu(&stage).run(&job, &mut want).expect("cpu cpi");
        let mut got = CubeOut::new(&stage.shape);
        backend.run_shared(&job, &mut got).expect("accelerated cpi");
        assert!(backend.gpu());
        broken.store(true, Ordering::Release);
        backend.run_shared(&job, &mut got).expect("recomputed cpi");
        assert!(!backend.gpu());
        assert_eq!(shared.gpu_failures.load(Ordering::Relaxed), 1);
        assert_eq!(got.cube, want.cube);
        backend.run_shared(&job, &mut got).expect("cpu cpi");
        assert_eq!(shared.gpu_failures.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_host_without_an_accelerator_keeps_the_cpu() {
        let stage = stage();
        let shared = Arc::new(Shared::default());
        assert!(matches!(offer(&stage, cpu(&stage), &shared), Offer::Cpu(_)));
    }
}

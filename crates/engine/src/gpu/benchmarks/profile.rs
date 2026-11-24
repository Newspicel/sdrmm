use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use sdrmm_array::passive_radar::{CubeOut, Illuminator};

use super::{
    FRAME,
    fft::Timer,
    radar::{job, kraken},
    samples,
};
use crate::gpu::{
    Context,
    caf::GpuCaf,
    compute::{Stage, Transfer},
    spectrum::Batch,
};

fn gpu_stages(timer: &Timer, context: &Context, stages: &[Stage<'_>]) -> (f64, String) {
    let times: Vec<(&str, f64)> = stages
        .iter()
        .map(|(name, stage)| (*name, timer.median_us(context, |pass| stage(pass))))
        .collect();
    let total = times.iter().map(|(_, us)| us).sum();
    let text = times
        .iter()
        .map(|(name, us)| format!("{name}={us:.0}"))
        .collect::<Vec<_>>()
        .join(" ");
    (total, text)
}

fn median(runs: usize, mut run: impl FnMut() -> Duration) -> f64 {
    let mut times: Vec<f64> = (0..runs).map(|_| run().as_secs_f64() * 1e6).collect();
    times.sort_by(f64::total_cmp);
    times[runs / 2]
}

/// Where a spectrum frame and a radar CPI spend their time: each GPU stage on the GPU clock,
/// the CPU work, and the whole call on the wall clock.
#[test]
#[ignore = "hardware benchmark"]
fn benchmark_profile() {
    let context = Arc::new(Context::new_profiling().expect("hardware GPU required"));
    let timer = Timer::new(&context);
    for (size, chunk) in [(4_096, 64), (65_536, 16), (262_144, 4)] {
        let hop = size / 2;
        let input = samples((chunk - 1) * hop + size, 7);
        let staged = Batch::with_transfer(context.clone(), size, chunk, Transfer::Staged).unwrap();
        let (gpu, stages) = gpu_stages(&timer, &context, &staged.stages());
        let mut batch = Batch::new(context.clone(), size, chunk).unwrap();
        let batches = FRAME / chunk;
        let mut db = vec![0.0; size];
        let wall = median(21, || {
            let start = Instant::now();
            for _ in 0..batches {
                batch.add(&input, hop, chunk).unwrap();
            }
            batch.take_db(FRAME as u32, &mut db).unwrap();
            start.elapsed()
        });
        let memcpy = median(21, || {
            let mut copy = input.clone();
            let start = Instant::now();
            for _ in 0..batches {
                copy.copy_from_slice(std::hint::black_box(&input));
            }
            std::hint::black_box(&copy);
            start.elapsed()
        });
        println!(
            "spectrum/{size},frame_wall_us={wall:.0},gpu_us={:.0} ({stages} per batch x{batches}),memcpy_us={memcpy:.0}",
            gpu * batches as f64
        );
    }
    for (name, illuminator, center) in [
        ("fm", Illuminator::Fm, 100e6),
        ("dab", Illuminator::Dab, 220e6),
    ] {
        let plan = kraken(center, illuminator);
        let job = job(&plan);
        let mut out = CubeOut::new(&plan.shape);
        let mut caf = GpuCaf::new(context.clone(), &plan.caf()).unwrap();
        caf.run(&job, &mut out).unwrap();
        let wall = median(11, || {
            let start = Instant::now();
            caf.run(&job, &mut out).unwrap();
            start.elapsed()
        });
        let staged = GpuCaf::with_transfer(context.clone(), &plan.caf(), Transfer::Staged).unwrap();
        let (gpu, stages) = gpu_stages(&timer, &context, &staged.stages());
        let upload = median(11, || caf.time_upload(&job).unwrap());
        let clutter = caf.time_clutter(&job).map_or_else(
            || "off".to_owned(),
            |(energies, solve)| {
                format!(
                    "energies {}us + solve {}us",
                    energies.as_micros(),
                    solve.as_micros()
                )
            },
        );
        println!(
            "caf/{name},wall_us={wall:.0},gpu_us={gpu:.0} ({stages}),upload_us={upload:.0},clutter_cpu={clutter},shape={:?}",
            plan.shape
        );
    }
}

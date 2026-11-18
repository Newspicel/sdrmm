use super::*;

fn roomy() -> wgpu::Limits {
    wgpu::Limits {
        max_buffer_size: 1 << 34,
        max_storage_buffer_binding_size: 1 << 34,
        ..wgpu::Limits::default()
    }
}

fn dab() -> BatchShape {
    BatchShape::new(1024, 1000, 548, 2, 103, 4).unwrap()
}

fn groups(batches: usize, per_group: usize, doppler_taps: usize) -> GroupPlan {
    GroupPlan::split(batches, per_group, 0, doppler_taps, true, false).unwrap()
}

#[test]
fn the_dab_default_fits_the_gpu_budget() {
    let plan = groups(1024, 100, 0);
    let footprint = Footprint::of(&dab(), Some(&plan)).unwrap();
    assert_eq!(footprint.spectra, 6 * 1024 * 2048 * 8);
    assert_eq!(footprint.cube, 4 * 548 * 1024 * 8);
    assert_eq!(footprint.window, 5 * dab().window() as u64 * 8);
    assert_eq!(footprint.sums, 10 * (1 + 4) * 2048 * 8);
    assert_eq!(footprint.weights, 10 * 4 * 2048 * 8);
    assert_eq!(footprint.turns, 10 * 1024 * 2 * 8);
    assert_eq!(footprint.fits(&roomy()), Ok(()));
}

#[test]
fn a_plan_over_512_mib_stays_off_the_gpu() {
    let shape = BatchShape::new(4096, 1000, 548, 2, 103, 15).unwrap();
    let footprint = Footprint::of(&shape, None).unwrap();
    assert!(footprint.spectra + footprint.cube > MAX_GPU_BYTES);
    assert!(footprint.fits(&roomy()).is_err());
}

#[test]
fn clutter_sums_count_toward_the_budget() {
    let shape = BatchShape::new(1024, 1000, 548, 2, 103, 15).unwrap();
    let coarse = Footprint::of(&shape, Some(&groups(1024, 100, 2))).unwrap();
    assert_eq!(coarse.fits(&roomy()), Ok(()));
    let fine = Footprint::of(&shape, Some(&groups(1024, 8, 2))).unwrap();
    assert!(fine.spectra + fine.cube < MAX_GPU_BYTES);
    assert!(fine.fits(&roomy()).is_err());
}

#[test]
fn a_buffer_over_the_binding_limit_stays_off_the_gpu() {
    let narrow = wgpu::Limits {
        max_storage_buffer_binding_size: 64 << 20,
        ..roomy()
    };
    assert!(Footprint::of(&dab(), None).unwrap().fits(&narrow).is_err());
}

#[test]
fn a_plan_without_clutter_needs_no_model_slot() {
    let shape = BatchShape::new(512, 260, 73, 0, 0, 4).unwrap();
    let footprint = Footprint::of(&shape, None).unwrap();
    assert_eq!(footprint.spectra, 5 * 512 * shape.fft_len as u64 * 8);
    assert_eq!(
        (footprint.sums, footprint.weights, footprint.turns),
        (0, 0, 0)
    );
    let layout = layout_of(&shape, None).unwrap();
    assert_eq!((layout.first_lane, layout.slots, layout.eca), (1, 5, 0));
}

mod against_the_cpu {
    use std::f64::consts::TAU;

    use num_complex::Complex;
    use sdrmm_array::GpuUse;
    use sdrmm_array::passive_radar::{
        CafBackend, ClutterMethod, ClutterParams, CpiJob, CpuCaf, CubeOut, Illuminator,
        PassiveRadarParams, RadarCtx, RadarPlan, ReferenceCleaning, plan,
    };

    use super::super::GpuCaf;

    type C32 = Complex<f32>;

    const KRAKEN_RATE: f64 = 2_400_000.0;
    const ELEMENTS: usize = 5;
    const ECHO_DELAY: usize = 40;
    const ECHO_HZ: f64 = 60.0;
    const TOLERANCE: f32 = 2e-4;

    fn stage(clutter: ClutterParams, illuminator: Illuminator) -> RadarPlan {
        let params = PassiveRadarParams {
            illuminator,
            clutter,
            aoa: false,
            reference: ReferenceCleaning::Off,
            gpu: GpuUse::Auto,
            ..PassiveRadarParams::default()
        };
        let ctx = RadarCtx {
            sample_rate: KRAKEN_RATE,
            center_hz: if illuminator == Illuminator::Dab {
                220e6
            } else {
                100e6
            },
            elements: ELEMENTS,
            positions_m: Vec::new(),
            manifold: None,
            tuned_together: true,
        };
        plan(&ctx, &params).expect("plan")
    }

    fn uniform(seed: u64) -> impl FnMut() -> f32 {
        let mut state = seed | 1;
        move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 40) as f32 / (1u64 << 24) as f32 - 0.5
        }
    }

    fn job(stage: &RadarPlan, seed: u64) -> CpiJob {
        let shape = stage.shape;
        let rate = stage.front.radar_rate;
        let mut next = uniform(seed);
        let mut job = CpiJob::new(shape.lanes + 1, shape.window());
        job.front_suppression_db = [7.5; 15];
        let mut phase = 0.0f32;
        let mut drive = 0.0f32;
        for sample in job.lane_mut(0) {
            drive = 0.995 * drive + 0.1 * next();
            phase += drive;
            *sample = C32::from_polar(1.0, phase);
        }
        let reference = job.lane(0).to_vec();
        for lane in 1..=shape.lanes {
            let gain = C32::from_polar(3.0, lane as f32);
            for (n, sample) in job.lane_mut(lane).iter_mut().enumerate() {
                let delayed = |delay: usize| {
                    n.checked_sub(delay)
                        .map_or(C32::default(), |at| reference[at])
                };
                let turn = (TAU * ECHO_HZ * n as f64 / rate) as f32;
                let direct = reference[n] * gain + delayed(3) * C32::new(0.4, -0.2);
                let echo = delayed(ECHO_DELAY) * C32::from_polar(0.03, turn);
                *sample = direct + echo + C32::new(next(), next()) * 0.05;
            }
        }
        job
    }

    fn on_the_cpu(stage: &RadarPlan, job: &CpiJob) -> CubeOut {
        let mut out = CubeOut::new(&stage.shape);
        CpuCaf::new(stage)
            .expect("cpu")
            .run(job, &mut out)
            .expect("cpu cpi");
        out
    }

    fn worst_error(want: &CubeOut, got: &CubeOut) -> f32 {
        let peak = want.cube.iter().map(|cell| cell.norm()).fold(0.0, f32::max);
        let worst = want
            .cube
            .iter()
            .zip(&got.cube)
            .map(|(a, b)| (a - b).norm())
            .fold(0.0, f32::max);
        worst / peak
    }

    fn on_the_gpu(stage: &RadarPlan) -> GpuCaf {
        let context = crate::gpu::context().expect("GPU adapter");
        GpuCaf::new(std::sync::Arc::clone(context), &stage.caf()).expect("GPU kernel")
    }

    #[test]
    #[ignore = "requires a GPU adapter"]
    fn gpu_matches_cpu_within_tolerance() {
        let methods = [
            (ClutterParams::default(), Illuminator::Fm),
            (
                ClutterParams {
                    method: ClutterMethod::EcaSliding,
                    doppler_taps: 1,
                    ..ClutterParams::default()
                },
                Illuminator::Fm,
            ),
            (
                ClutterParams {
                    taper: false,
                    doppler_taps: 2,
                    ..ClutterParams::default()
                },
                Illuminator::Fm,
            ),
            (
                ClutterParams {
                    method: ClutterMethod::Nlms,
                    ..ClutterParams::default()
                },
                Illuminator::Fm,
            ),
            (ClutterParams::default(), Illuminator::Dab),
        ];
        for (clutter, illuminator) in methods {
            let stage = stage(clutter, illuminator);
            let job = job(&stage, 11);
            let want = on_the_cpu(&stage, &job);
            let mut gpu = on_the_gpu(&stage);
            let mut got = CubeOut::new(&stage.shape);
            for _ in 0..2 {
                gpu.run(&job, &mut got).expect("gpu cpi");
            }
            let error = worst_error(&want, &got);
            assert!(error < TOLERANCE, "{illuminator:?} {clutter:?}: {error}");
            assert_eq!(got.unsuppressed_groups, want.unsuppressed_groups);
            for (lane, (a, b)) in got
                .suppression_db
                .iter()
                .zip(&want.suppression_db)
                .enumerate()
            {
                assert!((a - b).abs() < 0.1, "{clutter:?} lane {lane}: {a} vs {b}");
            }
        }
    }

    #[test]
    #[ignore = "requires a GPU adapter"]
    fn a_lost_gpu_buffer_fails_the_cpi() {
        let stage = stage(ClutterParams::default(), Illuminator::Fm);
        let job = job(&stage, 5);
        let mut gpu = on_the_gpu(&stage);
        let mut got = CubeOut::new(&stage.shape);
        gpu.run(&job, &mut got).expect("gpu cpi");
        gpu.sabotage();
        assert!(gpu.run(&job, &mut got).is_err());
    }
}

use std::{
    hint::black_box,
    sync::Arc,
    time::{Duration, Instant},
};

use num_complex::Complex;
use sdrmm_dsp::{PowerAverage, SpectrumAnalyzer, subband::SubbandPlan};

use super::{
    Context,
    spectrum::{Batch, Stream, batch_for},
};
use crate::spectrum::{Cadence, Tuning, display_window};

mod compute;
mod radar;
mod wideband;

pub(crate) fn samples(size: usize, seed: u32) -> Vec<Complex<f32>> {
    let mut state = seed;
    (0..size)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            Complex::new(
                (state as i32) as f32 / i32::MAX as f32,
                (state.rotate_left(13) as i32) as f32 / i32::MAX as f32,
            )
        })
        .collect()
}

pub(crate) fn measure(name: &str, mut run: impl FnMut()) {
    for _ in 0..8 {
        run();
    }
    let mut timings = Vec::with_capacity(101);
    for _ in 0..101 {
        let start = Instant::now();
        run();
        timings.push(start.elapsed().as_secs_f64() * 1e6);
    }
    timings.sort_by(f64::total_cmp);
    println!(
        "{name},median_us={:.3},p95_us={:.3}",
        timings[50], timings[95]
    );
}

const FRAME: usize = 64;

fn hardware() -> Arc<Context> {
    let context = Arc::new(Context::new().expect("hardware GPU required"));
    let info = context.device.adapter_info();
    println!(
        "adapter={},backend={:?},device={:?}",
        info.name, info.backend, info.device_type
    );
    if cfg!(target_os = "macos") {
        assert_eq!(info.backend, wgpu::Backend::Metal);
    }
    context
}

/// One frame of [`FRAME`] half-overlapping transforms: the old CPU path (dB per transform,
/// averaged back in power), the new CPU path (power summed directly) and GPU batches.
#[test]
#[ignore = "hardware benchmark"]
fn benchmark_spectrum_frame() {
    let context = hardware();
    for size in [4_096, 32_768, 65_536, 262_144] {
        let hop = size / 2;
        let input = samples((FRAME - 1) * hop + size, 0x1234567);
        let mut cpu = SpectrumAnalyzer::with_window(display_window(size));
        let mut average = PowerAverage::new(size);
        let mut db = vec![0.0; size];
        measure(&format!("frame/{size}/cpu_db"), || {
            for start in (0..FRAME).map(|j| j * hop) {
                cpu.power_db(black_box(&input[start..start + size]), &mut db);
                average.add(&db);
            }
            average.take_db(&mut db);
        });
        measure(&format!("frame/{size}/cpu_power"), || {
            for start in (0..FRAME).map(|j| j * hop) {
                cpu.accumulate(black_box(&input[start..start + size]), &mut average);
            }
            average.take_db(&mut db);
        });
        let default = batch_for(size);
        for chunk in [4, 8, 16, 32, 64].into_iter().filter(|&c| c <= default) {
            let mut batch = Batch::new(context.clone(), size, chunk).unwrap();
            measure(&format!("frame/{size}/gpu_batch{chunk}"), || {
                let mut done = 0;
                while done < FRAME {
                    let count = batch.fitting(hop).min(FRAME - done);
                    batch.add(&input[done * hop..], hop, count).unwrap();
                    done += count;
                }
                batch.take_db(FRAME as u32, &mut db).unwrap();
            });
        }
    }
}

/// Feeds 245.76 MS/s in real time through the GPU stream and reports what the DSP thread
/// spends on it and how many frames arrive.
#[test]
#[ignore = "hardware benchmark"]
fn benchmark_spectrum_stream() {
    let context = hardware();
    for (rate, size) in [
        (20_000_000.0, 32_768),
        (61_440_000.0, 65_536),
        (245_760_000.0, 262_144),
    ] {
        let budget = super::spectrum::budget(context.clone(), size).expect("GPU wins");
        let cadence = Cadence::new(rate, size, budget);
        let tuning = Tuning {
            center_hz: 1e9,
            span_hz: rate as f32,
        };
        let mut stream = Stream::new(context.clone(), size).unwrap();
        let block = samples(1 << 17, 0x1234567);
        let blocks = (rate / block.len() as f64) as usize;
        let mut frames = 0;
        let mut publish = |_: crate::spectrum::SpectrumFrame, db: &[f32]| {
            black_box(db);
            frames += 1;
        };
        let mut busy = Duration::ZERO;
        let start = Instant::now();
        for index in 0..blocks {
            let due = start + Duration::from_secs_f64((index * block.len()) as f64 / rate);
            while Instant::now() < due {
                std::hint::spin_loop();
            }
            let began = Instant::now();
            let at = (index * block.len()) as u64;
            stream
                .push(&block, at, tuning, cadence, &mut publish)
                .unwrap();
            busy += began.elapsed();
        }
        let wall = start.elapsed();
        std::thread::sleep(Duration::from_millis(100));
        stream.idle(&mut publish).unwrap();
        println!(
            "stream/{rate}/{size},budget={budget:.3e},hop={},averages={},frames={frames},wall_s={:.3},dsp_busy_pct={:.2}",
            cadence.hop,
            cadence.averages,
            wall.as_secs_f64(),
            100.0 * busy.as_secs_f64() / wall.as_secs_f64(),
        );
    }
}

#[test]
#[ignore = "hardware benchmark"]
fn benchmark_wideband() {
    let context = Arc::new(Context::new().expect("hardware GPU required"));
    let plan = SubbandPlan::new(20_000_000.0).unwrap();
    for size in [2048, 4096, 8192, 32_768] {
        let input = samples(size, 0x1234567);
        let mut cpu = plan.filter_bank(size);
        let mut gpu = wideband::Wideband::new(context.clone(), size);
        let mut actual = vec![0.0; size.div_ceil(5) * 13 * 2];
        for _ in 0..5 {
            cpu.process(&input);
            let frames = gpu.process(&input, &mut actual);
            for band in 0..13 {
                assert_eq!(cpu.samples(band).len(), frames);
                for (sample, expected) in cpu.samples(band).iter().enumerate() {
                    let at = (band * frames + sample) * 2;
                    let actual = Complex::new(actual[at], actual[at + 1]);
                    assert!((actual - expected).norm() < 2e-6, "wideband mismatch");
                }
            }
        }
        measure(&format!("wideband/{size}/cpu"), || {
            cpu.process(black_box(&input));
            black_box(cpu.samples(0));
        });
        measure(&format!("wideband/{size}/gpu"), || {
            gpu.process(black_box(&input), &mut actual);
            black_box(&actual);
        });
    }
}

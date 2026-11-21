use std::{hint::black_box, time::Instant};

use num_complex::Complex;
use sdrmm_dsp::math::fft::FftPair;
use wgpu::util::DeviceExt;

use super::samples;
use crate::gpu::{
    Context, buffer,
    compute::Kernel,
    fft::{FftBatch, Options, Transforms, Twiddles},
};

const POINTS: usize = 1 << 21;
const RUNS: u32 = 32;

/// Times compute passes on the GPU clock, so submission and readback stay out of the figure.
pub(super) struct Timer {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    period_ns: f64,
}

impl Timer {
    pub(super) fn new(context: &Context) -> Self {
        let bytes = u64::from(RUNS) * 16;
        Self {
            set: context.device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("FFT timer"),
                ty: wgpu::QueryType::Timestamp,
                count: RUNS * 2,
            }),
            resolve: buffer(
                &context.device,
                "FFT timer resolve",
                bytes,
                wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            ),
            read: buffer(
                &context.device,
                "FFT timer read",
                bytes,
                wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            ),
            period_ns: f64::from(context.queue.get_timestamp_period()),
        }
    }

    /// Median GPU microseconds of `record` over [`RUNS`] passes.
    pub(super) fn median_us(
        &self,
        context: &Context,
        record: impl Fn(&mut wgpu::ComputePass<'_>),
    ) -> f64 {
        warm_up(context, &record);
        let mut encoder = context.device.create_command_encoder(&Default::default());
        for run in 0..RUNS {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                    query_set: &self.set,
                    beginning_of_pass_write_index: Some(run * 2),
                    end_of_pass_write_index: Some(run * 2 + 1),
                }),
            });
            record(&mut pass);
        }
        encoder.resolve_query_set(&self.set, 0..RUNS * 2, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, self.read.size());
        context.queue.submit([encoder.finish()]);
        self.read
            .map_async(wgpu::MapMode::Read, .., |result| result.expect("mapped"));
        context
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("GPU finished");
        let mut times: Vec<f64> = {
            let view = self.read.get_mapped_range(..).expect("timestamps");
            bytemuck::cast_slice::<u8, u64>(&view)
                .chunks(2)
                .map(|pair| pair[1].saturating_sub(pair[0]) as f64 * self.period_ns / 1e3)
                .collect()
        };
        self.read.unmap();
        times.sort_by(f64::total_cmp);
        times[times.len() / 2]
    }
}

/// Keeps the GPU busy for a while first, so its clock has ramped up before timing starts.
fn warm_up(context: &Context, record: &impl Fn(&mut wgpu::ComputePass<'_>)) {
    let start = Instant::now();
    while start.elapsed().as_millis() < 30 {
        let mut encoder = context.device.create_command_encoder(&Default::default());
        for _ in 0..RUNS {
            record(&mut encoder.begin_compute_pass(&Default::default()));
        }
        context.queue.submit([encoder.finish()]);
        context
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("GPU finished");
    }
}

fn median_us(runs: usize, mut run: impl FnMut()) -> f64 {
    run();
    let mut times: Vec<f64> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            run();
            start.elapsed().as_secs_f64() * 1e6
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times[runs / 2]
}

fn storage(context: &Context, values: &[Complex<f32>]) -> wgpu::Buffer {
    context
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("FFT benchmark data"),
            contents: bytemuck::cast_slice(values),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        })
}

/// Upload, transform and read back one batch, as a caller without a resident pipeline pays.
fn round_trip_us(
    context: &Context,
    fft: &FftBatch,
    data: &wgpu::Buffer,
    input: &[Complex<f32>],
) -> f64 {
    let bytes = std::mem::size_of_val(input) as u64;
    let readback = buffer(
        &context.device,
        "FFT benchmark readback",
        bytes,
        wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
    );
    median_us(11, || {
        context
            .queue
            .write_buffer(data, 0, bytemuck::cast_slice(input));
        let mut encoder = context.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            fft.dispatch(&mut pass);
        }
        encoder.copy_buffer_to_buffer(data, 0, &readback, 0, bytes);
        context.queue.submit([encoder.finish()]);
        readback.map_async(wgpu::MapMode::Read, .., |result| result.expect("mapped"));
        context
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("GPU finished");
        black_box(&readback.get_mapped_range(..).expect("mapped range")[..8]);
        readback.unmap();
    })
}

fn cpu_us(size: usize, input: &[Complex<f32>]) -> f64 {
    let mut pair = FftPair::new(size);
    let mut work = input.to_vec();
    median_us(5, || {
        for chunk in work.chunks_mut(size) {
            pair.forward(black_box(chunk));
        }
    })
}

fn stages_us<'a>(
    timer: &Timer,
    context: &Context,
    kernels: impl Iterator<Item = &'a Kernel>,
) -> String {
    kernels
        .map(|kernel| {
            format!(
                "{:.1}",
                timer.median_us(context, |pass| kernel.dispatch(pass))
            )
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// GPU time per batch from timestamps, each kernel's share, the full upload and readback
/// round trip, and single-threaded rustfft over the same batch.
#[test]
#[ignore = "hardware benchmark"]
fn benchmark_fft_kernels() {
    let context = Context::new_profiling().expect("hardware GPU required");
    assert!(
        context
            .device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY),
        "adapter lacks timestamp queries"
    );
    println!("adapter={}", context.adapter_name());
    let timer = Timer::new(&context);
    for bits in [6, 8, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 22] {
        let size = 1usize << bits;
        let batches = (POINTS / size).max(1);
        let input = samples(size * batches, bits);
        let data = storage(&context, &input);
        let fft = FftBatch::new(&context, &data, size, batches, false).expect("fft");
        let gpu = timer.median_us(&context, |pass| fft.dispatch(pass));
        let stages = stages_us(&timer, &context, fft.kernels());
        let trip = round_trip_us(&context, &fft, &data, &input);
        let cpu = cpu_us(size, &input);
        let points = (size * batches) as f64;
        println!(
            "fft/{size}x{batches},gpu_us={gpu:.1},gpu_gbps={:.0},stages_us={stages},round_trip_us={trip:.0},cpu_us={cpu:.0},gpu_vs_cpu={:.1}x,trip_vs_cpu={:.1}x",
            points * 16.0 / gpu / 1e3,
            cpu / gpu,
            cpu / trip,
        );
    }
}

/// GPU time per batch for every radix, tile and twiddle choice.
#[test]
#[ignore = "hardware benchmark"]
fn benchmark_fft_options() {
    let context = Context::new_profiling().expect("hardware GPU required");
    let timer = Timer::new(&context);
    for bits in [13, 14, 15, 16, 17, 18, 19, 20, 21, 22] {
        let size = 1usize << bits;
        let batches = (POINTS / size).max(1);
        let data = storage(&context, &samples(size * batches, bits));
        let plan = Transforms {
            size,
            offset: 0,
            batches,
            inverse: false,
        };
        let mut row = Vec::new();
        for radix in [4, 8, 16] {
            for (span, columns) in [(4096, 1), (4096, 4), (2048, 1), (1024, 1)] {
                let options = Options {
                    radix,
                    span,
                    columns,
                    twiddles: Twiddles::Computed,
                };
                let fft = FftBatch::with_options(&context, &data, plan, options).expect("fft");
                let gpu = timer.median_us(&context, |pass| fft.dispatch(pass));
                row.push((gpu, format!("r{radix}/s{span}/c{columns}")));
            }
        }
        row.sort_by(|a, b| a.0.total_cmp(&b.0));
        let best: Vec<String> = row
            .iter()
            .take(4)
            .map(|(us, name)| format!("{name}={us:.1}"))
            .collect();
        let worst = row
            .last()
            .map_or_else(String::new, |(us, name)| format!("{name}={us:.1}"));
        println!(
            "options/{size}x{batches},best={},worst={worst}",
            best.join(" ")
        );
    }
}

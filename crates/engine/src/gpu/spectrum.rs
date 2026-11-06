use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use num_complex::Complex;
use sdrmm_dsp::coherent_gain;

use super::{
    Context, buffer, bytes_for,
    compute::{Kernel, Readback, compile, grid, initialized, module},
    fft::{FftBatch, check_size},
};

mod stream;

pub(crate) use stream::Stream;

const THREADS: u64 = 256;
/// Complex elements one batch aims to transform per submission. Fewer leave an M4 Max idle
/// between dispatches; more stop paying off and cost memory.
const BATCH_ELEMENTS: usize = 1 << 20;
const MIN_BATCH: usize = 8;
const MAX_BATCH: usize = 64;
/// Share of the GPU the display may take, so rendering and other work keep the rest.
const GPU_SHARE: f64 = 0.25;
const CPU_PROBE: u32 = 8;
const HOP_WORD: u64 = 8;
const AVERAGED_WORD: u64 = 16;

/// Windows, transforms and sums the power of up to `chunk` overlapping transforms per
/// submission, then reads back one averaged spectrum in dB.
pub(crate) struct Batch {
    context: Arc<Context>,
    size: usize,
    chunk: usize,
    input: wgpu::Buffer,
    params: wgpu::Buffer,
    levels: wgpu::Buffer,
    prepare: Kernel,
    fft: FftBatch,
    accumulate: Kernel,
    finalize: Kernel,
    readback: Readback,
}

pub(crate) fn batch_for(size: usize) -> usize {
    (BATCH_ELEMENTS / size.max(1)).clamp(MIN_BATCH, MAX_BATCH)
}

impl Batch {
    pub(crate) fn new(context: Arc<Context>, size: usize, chunk: usize) -> Result<Self, String> {
        check_size(size)?;
        let chunk = chunk.max(1);
        let elements = size
            .checked_mul(chunk)
            .ok_or_else(|| "GPU spectrum batch overflow".to_owned())?;
        let complex_bytes = bytes_for::<[f32; 2]>(elements)?;
        if complex_bytes > context.device.limits().max_storage_buffer_binding_size {
            return Err(format!(
                "a GPU spectrum batch of {chunk} x {size} exceeds the binding limit"
            ));
        }
        let narrow = |value: usize| {
            u32::try_from(value).map_err(|_| format!("GPU spectrum index {value} exceeds 32 bits"))
        };
        let window = crate::spectrum::display_window(size);
        let inv_gain = 1.0 / coherent_gain(&window).max(f32::MIN_POSITIVE);
        let scale = (inv_gain * inv_gain).to_bits();
        let device = &context.device;
        let storage = wgpu::BufferUsages::STORAGE;
        let input = buffer(
            device,
            "SDRmm spectrum input",
            complex_bytes,
            storage | wgpu::BufferUsages::COPY_DST,
        );
        let data = buffer(device, "SDRmm spectrum transforms", complex_bytes, storage);
        let levels = buffer(
            device,
            "SDRmm spectrum levels",
            bytes_for::<f32>(size)?,
            storage | wgpu::BufferUsages::COPY_SRC,
        );
        let params = initialized(
            &context,
            &[narrow(size)?, size.trailing_zeros(), 0, 0, 0, scale],
        );
        let window = initialized(&context, &window);
        let power = initialized(&context, &vec![0.0f32; size]);
        let shader = module(&context, include_str!("spectrum.wgsl"), "spectrum");
        let kernel = |entry: &str, buffers: &[(u32, &wgpu::Buffer)], threads: usize| {
            let pipeline = compile(&context, &shader, entry);
            Ok::<_, String>(Kernel::bind(
                &context,
                &pipeline,
                buffers,
                grid(threads as u64, THREADS)?,
            ))
        };
        let prepare = kernel(
            "prepare",
            &[(0, &input), (1, &data), (2, &params), (3, &window)],
            elements,
        )?;
        let accumulate = kernel("accumulate", &[(1, &data), (2, &params), (4, &power)], size)?;
        let finalize = kernel("finalize", &[(2, &params), (4, &power), (5, &levels)], size)?;
        let fft = FftBatch::new(&context, &data, size, chunk, false)?;
        let readback = Readback::new(&context, "SDRmm spectrum readback", bytes_for::<f32>(size)?);
        Ok(Self {
            context,
            size,
            chunk,
            input,
            params,
            levels,
            prepare,
            fft,
            accumulate,
            finalize,
            readback,
        })
    }

    pub(crate) const fn size(&self) -> usize {
        self.size
    }

    pub(crate) const fn capacity(&self) -> usize {
        self.size * self.chunk
    }

    /// How many transforms `hop` apart one submission holds.
    pub(crate) fn fitting(&self, hop: usize) -> usize {
        ((self.capacity() - self.size) / hop.max(1) + 1).min(self.chunk)
    }

    /// Adds the power of `count` transforms, `hop` samples apart, to the running sum.
    pub(crate) fn add(
        &mut self,
        samples: &[Complex<f32>],
        hop: usize,
        count: usize,
    ) -> Result<(), String> {
        let span = count
            .checked_sub(1)
            .and_then(|gaps| gaps.checked_mul(hop))
            .and_then(|start| start.checked_add(self.size))
            .filter(|&span| count <= self.chunk && span <= self.capacity())
            .ok_or_else(|| format!("{count} transforms {hop} apart do not fit a GPU batch"))?;
        let span_samples = samples
            .get(..span)
            .ok_or_else(|| format!("{} samples cannot fill {span}", samples.len()))?;
        let words = [hop.min(span), count].map(|value| value as u32);
        let queue = &self.context.queue;
        queue.write_buffer(&self.params, HOP_WORD, bytemuck::cast_slice(&words));
        queue.write_buffer(&self.input, 0, bytemuck::cast_slice(span_samples));
        self.submit(|pass, batch| {
            batch.prepare.dispatch(pass);
            batch.fft.dispatch(pass);
            batch.accumulate.dispatch(pass);
        });
        Ok(())
    }

    /// Reads back the mean of `averaged` transforms in dB and clears the sum.
    pub(crate) fn take_db(&mut self, averaged: u32, out: &mut [f32]) -> Result<(), String> {
        if out.len() != self.size {
            return Err(format!(
                "a spectrum of {} bins cannot take {}",
                out.len(),
                self.size
            ));
        }
        self.context.queue.write_buffer(
            &self.params,
            AVERAGED_WORD,
            bytemuck::bytes_of(&averaged.max(1)),
        );
        let mut encoder = self
            .context
            .device
            .create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            self.finalize.dispatch(&mut pass);
        }
        self.readback.copy(&mut encoder, &self.levels);
        let submission = self.context.queue.submit([encoder.finish()]);
        self.readback.wait(&self.context, submission)?;
        self.readback.read::<f32>(|levels| {
            out.copy_from_slice(levels);
            Ok(())
        })
    }

    /// Drops a partial sum without reading it back.
    pub(crate) fn clear(&mut self) {
        self.submit(|pass, batch| batch.finalize.dispatch(pass));
    }

    fn submit(&self, record: impl FnOnce(&mut wgpu::ComputePass<'_>, &Self)) {
        let mut encoder = self
            .context
            .device
            .create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            record(&mut pass, self);
        }
        self.context.queue.submit([encoder.finish()]);
    }
}

/// Times a frame of [`MAX_BATCH`] transforms both ways. When the GPU wins clearly, returns
/// the transformed samples per second it can take within [`GPU_SHARE`], so a slower adapter
/// averages fewer transforms instead of falling behind.
pub(crate) fn budget(context: Arc<Context>, size: usize) -> Option<f64> {
    match measure(context, size) {
        Ok((cpu, gpu)) => {
            let faster = gpu.as_secs_f64() * 1.2 < cpu.as_secs_f64();
            let budget = GPU_SHARE * (MAX_BATCH * size) as f64 / gpu.as_secs_f64().max(1e-9);
            tracing::info!(
                size,
                cpu_us = cpu.as_micros(),
                gpu_us = gpu.as_micros(),
                faster,
                budget,
                "spectrum backend timing per frame"
            );
            faster.then_some(budget)
        }
        Err(error) => {
            tracing::warn!(%error, "GPU spectrum timing failed; using CPU");
            None
        }
    }
}

fn measure(context: Arc<Context>, size: usize) -> Result<(Duration, Duration), String> {
    let mut batch = Batch::new(context, size, batch_for(size))?;
    let hop = size / 2;
    let input = probe_signal(batch.capacity());
    let mut cpu = crate::spectrum::cpu_analyzer(size);
    let mut average = sdrmm_dsp::PowerAverage::new(size);
    let mut out = vec![0.0; size];
    let mut gpu_frame = || -> Result<(), String> {
        let mut done = 0;
        while done < MAX_BATCH {
            let count = batch.fitting(hop).min(MAX_BATCH - done);
            batch.add(&input, hop, count)?;
            done += count;
        }
        batch.take_db(MAX_BATCH as u32, &mut out)
    };
    gpu_frame()?;
    let gpu = median(5, gpu_frame)?;
    let window = &input[..size];
    let probe = median(5, || {
        for _ in 0..CPU_PROBE {
            cpu.accumulate(std::hint::black_box(window), &mut average);
        }
        average.reset();
        Ok(())
    })?;
    Ok((probe * (MAX_BATCH as u32 / CPU_PROBE), gpu))
}

fn median(runs: usize, mut run: impl FnMut() -> Result<(), String>) -> Result<Duration, String> {
    let mut times = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        run()?;
        times.push(start.elapsed());
    }
    times.sort_unstable();
    Ok(times[runs / 2])
}

fn probe_signal(len: usize) -> Vec<Complex<f32>> {
    (0..len)
        .map(|index| {
            Complex::new(
                (index % 127) as f32 / 127.0 - 0.5,
                (index % 257) as f32 / 257.0 - 0.5,
            )
        })
        .collect()
}

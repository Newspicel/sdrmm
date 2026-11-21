use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use num_complex::Complex;
use sdrmm_array::kernel::{CafKernel, CafPlan, CpiJob, CubeOut};
use sdrmm_dsp::radar::{
    batch::BatchShape,
    wiener::{GroupPlan, GroupSums, MixTerm, WeightTable, WienerSolver},
};
use wgpu::util::DeviceExt;

use super::{
    Context, buffer,
    compute::{Kernel, Readback, Transfer, Upload, compile, grid, initialized, module},
    fft::{FftBatch, Loader, Transforms, check_size},
};

const SHAPE_WGSL: &str = include_str!("caf_shape.wgsl");

type C32 = Complex<f32>;

pub(crate) const MAX_GPU_BYTES: u64 = 512 << 20;
const THREADS: u64 = 256;
const COMPLEX_BYTES: u64 = 8;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct Layout {
    fft_len: u32,
    batches: u32,
    batch_len: u32,
    gates: u32,
    lead: u32,
    span: u32,
    order: u32,
    pre: u32,
    window: u32,
    lanes: u32,
    first_lane: u32,
    slots: u32,
    groups: u32,
    shifts: u32,
    taps: u32,
    vectors: u32,
    eca: u32,
    scale: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct Term {
    group: u32,
    tap: u32,
    factor: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Footprint {
    pub(crate) window: u64,
    pub(crate) spectra: u64,
    pub(crate) cube: u64,
    pub(crate) sums: u64,
    pub(crate) weights: u64,
    pub(crate) turns: u64,
}

impl Footprint {
    pub(crate) fn of(shape: &BatchShape, groups: Option<&GroupPlan>) -> Result<Self, String> {
        let bytes = |factors: &[usize]| {
            factors
                .iter()
                .try_fold(COMPLEX_BYTES, |total, &factor| {
                    total.checked_mul(u64::try_from(factor).ok()?)
                })
                .ok_or_else(|| "GPU buffer size overflow".to_owned())
        };
        let (count, shifts, taps) = groups.map_or((0, 0, 0), |plan| {
            (plan.groups(), plan.shifts(), plan.taps())
        });
        Ok(Self {
            window: bytes(&[shape.lanes + 1, shape.window()])?,
            spectra: bytes(&[slots(shape), shape.batches, shape.fft_len])?,
            cube: bytes(&[shape.lanes, shape.gates, shape.batches])?,
            sums: bytes(&[count, shifts + shape.lanes * taps, shape.fft_len])?,
            weights: bytes(&[count, shape.lanes, taps, shape.fft_len])?,
            turns: bytes(&[count, shape.batches, shifts + taps])?,
        })
    }

    pub(crate) fn fits(&self, limits: &wgpu::Limits) -> Result<(), String> {
        let budget = MAX_GPU_BYTES.min(limits.max_buffer_size);
        let needed = [self.spectra, self.cube, self.sums, self.weights, self.turns]
            .into_iter()
            .fold(0u64, u64::saturating_add);
        if needed > budget {
            return Err(format!(
                "the CAF needs {} MiB of GPU memory, more than {} MiB",
                needed >> 20,
                budget >> 20
            ));
        }
        let binding = limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size);
        let buffers = [
            self.window,
            self.spectra,
            self.cube,
            self.sums,
            self.weights,
            self.turns,
        ];
        if buffers.into_iter().any(|bytes| bytes > binding) {
            return Err("a CAF buffer exceeds the GPU binding limit".to_owned());
        }
        Ok(())
    }
}

const fn slots(shape: &BatchShape) -> usize {
    1 + shape.eca() as usize + shape.lanes
}

struct Eca {
    solver: WienerSolver,
    sums: GroupSums,
    table: WeightTable,
    per_group: usize,
    energy: Vec<f64>,
    totals: wgpu::Buffer,
    readback: Readback,
    weights: wgpu::Buffer,
    kernel: Kernel,
}

impl Eca {
    fn new(
        context: &Context,
        plan: &CafPlan<'_>,
        groups: &GroupPlan,
        pipeline: &wgpu::ComputePipeline,
        bound: &Bound<'_>,
    ) -> Result<(Self, wgpu::Buffer, wgpu::Buffer), String> {
        let shape = plan.shape;
        let solver = WienerSolver::new(shape, groups, plan.loading)
            .map_err(|error| format!("clutter solver: {error}"))?;
        let sums = GroupSums::new(&shape, groups);
        let table =
            WeightTable::new(&shape, groups).map_err(|error| format!("weight table: {error}"))?;
        let values = groups.groups() * sums.vectors() * shape.fft_len;
        let bytes = values as u64 * COMPLEX_BYTES;
        let totals = buffer(
            &context.device,
            "SDRmm CAF group sums",
            bytes,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );
        let weights = buffer(
            &context.device,
            "SDRmm CAF weights",
            table.spectra().len() as u64 * COMPLEX_BYTES,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let turns = initialized(context, &turns_of(&solver, &shape, groups)?);
        let ranges = initialized(context, &ranges_of(&solver, groups)?);
        let terms = initialized(context, &terms_of(&table, &shape, groups)?);
        let kernel = Kernel::bind(
            context,
            pipeline,
            &[
                (0, bound.layout),
                (2, bound.spectra),
                (3, &totals),
                (4, &turns),
                (5, &ranges),
            ],
            grid(values as u64, THREADS)?,
        );
        let eca = Self {
            solver,
            per_group: sums.vectors() * shape.fft_len,
            sums,
            table,
            energy: vec![0.0; shape.lanes],
            totals,
            readback: Readback::new(context, "SDRmm CAF group sums readback", bytes),
            weights: weights.clone(),
            kernel,
        };
        Ok((eca, weights, terms))
    }

    fn energies(&mut self, shape: &BatchShape, job: &CpiJob) -> Result<(), String> {
        self.sums.clear();
        for batch in 0..shape.batches {
            for (lane, energy) in self.energy.iter_mut().enumerate() {
                *energy = shape.energy(job.lane(lane + 1), batch);
            }
            self.solver
                .accumulate_energy(&mut self.sums, batch, &self.energy)
                .map_err(|error| format!("clutter energy: {error}"))?;
        }
        Ok(())
    }

    fn load(&mut self) -> Result<(), String> {
        let Self {
            readback,
            per_group,
            sums,
            ..
        } = self;
        readback.read(|values| {
            for (group, chunk) in values.chunks_exact(*per_group).enumerate() {
                sums.load(group, chunk)
                    .map_err(|error| format!("group sums: {error}"))?;
            }
            Ok(())
        })
    }
}

struct Bound<'a> {
    layout: &'a wgpu::Buffer,
    spectra: &'a wgpu::Buffer,
}

struct Stages {
    forward: FftBatch,
    inverse: FftBatch,
    gather: Kernel,
    doppler: FftBatch,
    shift: Kernel,
}

pub(crate) struct GpuCaf {
    context: Arc<Context>,
    shape: BatchShape,
    upload: Upload,
    cube: wgpu::Buffer,
    readback: Readback,
    stages: Stages,
    eca: Option<Eca>,
}

impl CafKernel for GpuCaf {
    fn run(&mut self, job: &CpiJob, out: &mut CubeOut) -> Result<(), String> {
        Self::run(self, job, out)
    }
}

impl GpuCaf {
    pub(crate) fn new(context: Arc<Context>, plan: &CafPlan<'_>) -> Result<Self, String> {
        let transfer = Transfer::fitting(&context);
        Self::with_transfer(context, plan, transfer)
    }

    pub(crate) fn with_transfer(
        context: Arc<Context>,
        plan: &CafPlan<'_>,
        transfer: Transfer,
    ) -> Result<Self, String> {
        let shape = plan.shape;
        check_size(shape.fft_len)?;
        check_size(shape.batches)?;
        if plan.window.len() != shape.batches {
            return Err("Doppler window does not match the plan".to_owned());
        }
        let groups = plan.groups.filter(|_| shape.eca());
        let footprint = Footprint::of(&shape, groups)?;
        footprint.fits(&context.device.limits())?;
        let device = context.device.clone();
        guarded(&device, || Self::build(context, plan, footprint, transfer))
    }

    fn build(
        context: Arc<Context>,
        plan: &CafPlan<'_>,
        footprint: Footprint,
        transfer: Transfer,
    ) -> Result<Self, String> {
        let shape = plan.shape;
        let storage = wgpu::BufferUsages::STORAGE;
        let spectra = buffer(
            &context.device,
            "SDRmm CAF spectra",
            footprint.spectra,
            storage,
        );
        let cube = buffer(
            &context.device,
            "SDRmm CAF cube",
            footprint.cube,
            storage | wgpu::BufferUsages::COPY_SRC,
        );
        let taper = initialized(&context, plan.window);
        let source = [SHAPE_WGSL, include_str!("caf_batch.wgsl")].concat();
        let module = module(&context, &source, "caf batch");
        let pipeline = |entry: &str| compile(&context, &module, entry);
        let groups = plan.groups.filter(|_| shape.eca());
        let layout = layout_of(&shape, groups)?;
        let layout = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("SDRmm CAF layout"),
                contents: bytemuck::bytes_of(&layout),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bound = Bound {
            layout: &layout,
            spectra: &spectra,
        };
        let (eca, weights, terms) = match groups {
            Some(groups) => {
                let (eca, weights, terms) =
                    Eca::new(&context, plan, groups, &pipeline("group_sums"), &bound)?;
                (Some(eca), weights, terms)
            }
            None => (
                None,
                initialized(&context, &[[0.0f32; 2]]),
                initialized(&context, &[Term::default()]),
            ),
        };
        let (m, nb, k, r) = (shape.fft_len, shape.batches, shape.lanes, shape.gates);
        let upload = Upload::new(&context, "SDRmm CAF samples", footprint.window, 1, transfer);
        let first_lane = 1 + usize::from(shape.eca());
        let count = |factors: &[usize]| factors.iter().product::<usize>() as u64;
        let stages = Stages {
            forward: FftBatch::loaded(
                &context,
                &spectra,
                Transforms {
                    size: m,
                    offset: 0,
                    batches: slots(&shape) * nb,
                    inverse: false,
                },
                &Loader {
                    wgsl: &[SHAPE_WGSL, include_str!("caf_load.wgsl")].concat(),
                    bindings: upload
                        .inputs()
                        .into_iter()
                        .map(|input| vec![(3, &layout), (4, input)])
                        .collect(),
                    in_place: false,
                },
            )?,
            inverse: FftBatch::loaded(
                &context,
                &spectra,
                Transforms {
                    size: m,
                    offset: first_lane * nb * m,
                    batches: k * nb,
                    inverse: true,
                },
                &Loader {
                    wgsl: &[SHAPE_WGSL, include_str!("caf_residual.wgsl")].concat(),
                    bindings: vec![vec![(3, &layout), (4, &weights), (5, &terms)]],
                    in_place: true,
                },
            )?,
            gather: Kernel::bind(
                &context,
                &pipeline("gather"),
                &[(0, &layout), (2, &spectra), (8, &cube), (9, &taper)],
                grid(count(&[k, r, nb]), THREADS)?,
            ),
            doppler: FftBatch::over(
                &context,
                &cube,
                Transforms {
                    size: nb,
                    offset: 0,
                    batches: k * r,
                    inverse: false,
                },
            )?,
            shift: Kernel::bind(
                &context,
                &pipeline("shift"),
                &[(0, &layout), (8, &cube)],
                grid(count(&[k, r, nb / 2]), THREADS)?,
            ),
        };
        let readback = Readback::new(&context, "SDRmm CAF cube readback", footprint.cube);
        Ok(Self {
            context,
            shape,
            upload,
            cube,
            readback,
            stages,
            eca,
        })
    }

    pub(crate) fn run(&mut self, job: &CpiJob, out: &mut CubeOut) -> Result<(), String> {
        let shape = self.shape;
        let fits = job.lanes == shape.lanes + 1
            && job.window >= shape.window()
            && job.samples.len() >= job.lanes * job.window
            && out.cube.len() == shape.lanes * shape.gates * shape.batches;
        if !fits {
            return Err("CPI does not match the GPU plan".to_owned());
        }
        let context = Arc::clone(&self.context);
        guarded(&context.device, || self.compute(job, out))
    }

    fn compute(&mut self, job: &CpiJob, out: &mut CubeOut) -> Result<(), String> {
        let mut encoder = self.encoder();
        let input = self.write_samples(job, &mut encoder)?;
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            self.stages.forward.dispatch_from(&mut pass, input);
            if let Some(eca) = &self.eca {
                eca.kernel.dispatch(&mut pass);
            }
        }
        let encoder = if let Some(eca) = self.eca.as_mut() {
            eca.readback.copy(&mut encoder, &eca.totals);
            let submission = self.context.queue.submit([encoder.finish()]);
            self.upload.submitted();
            eca.energies(&self.shape, job)?;
            eca.readback.wait(&self.context, submission)?;
            eca.load()?;
            let stats = eca
                .solver
                .solve(&eca.sums, &mut eca.table)
                .map_err(|error| format!("clutter solve: {error}"))?;
            out.suppression_db = stats.suppression_db;
            out.unsuppressed_groups = stats.unsuppressed_groups;
            write_complex(&self.context, &eca.weights, eca.table.spectra())?;
            self.encoder()
        } else {
            out.suppression_db = job.front_suppression_db;
            out.unsuppressed_groups = 0;
            encoder
        };
        self.finish(encoder, out)?;
        self.upload.submitted();
        Ok(())
    }

    fn finish(&self, mut encoder: wgpu::CommandEncoder, out: &mut CubeOut) -> Result<(), String> {
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            self.stages.inverse.dispatch(&mut pass);
            self.stages.gather.dispatch(&mut pass);
            self.stages.doppler.dispatch(&mut pass);
            self.stages.shift.dispatch(&mut pass);
        }
        self.readback.copy(&mut encoder, &self.cube);
        let submission = self.context.queue.submit([encoder.finish()]);
        self.readback.wait(&self.context, submission)?;
        self.readback.read(|values| {
            if values.len() != out.cube.len() {
                return Err("GPU cube length mismatch".to_owned());
            }
            out.cube.copy_from_slice(values);
            Ok(())
        })
    }

    fn write_samples(
        &self,
        job: &CpiJob,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<usize, String> {
        let width = self.shape.window();
        let lanes: Vec<&[u8]> = (0..job.lanes)
            .map(|lane| bytemuck::cast_slice(job.lane(lane).get(..width).unwrap_or_default()))
            .collect();
        let bytes: usize = lanes.iter().map(|lane| lane.len()).sum();
        if bytes as u64 != self.upload.bytes() {
            return Err("GPU upload is short".to_owned());
        }
        self.upload.write(&self.context, encoder, &lanes)
    }

    fn encoder(&self) -> wgpu::CommandEncoder {
        self.context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("SDRmm CAF"),
            })
    }

    #[cfg(test)]
    pub(crate) fn sabotage(&self) {
        self.cube.destroy();
    }

    #[cfg(test)]
    pub(crate) fn stages(&self) -> Vec<super::compute::Stage<'_>> {
        fn stage<'a>(
            name: &'static str,
            run: impl Fn(&mut wgpu::ComputePass<'_>) + 'a,
        ) -> super::compute::Stage<'a> {
            (name, Box::new(run))
        }
        let s = &self.stages;
        let mut out = vec![stage("forward fft", |pass| s.forward.dispatch(pass))];
        if let Some(eca) = &self.eca {
            out.push(stage("clutter sums", |pass| eca.kernel.dispatch(pass)));
        }
        out.extend([
            stage("residual + inverse fft", |pass| s.inverse.dispatch(pass)),
            stage("gather", |pass| s.gather.dispatch(pass)),
            stage("doppler fft", |pass| s.doppler.dispatch(pass)),
            stage("shift", |pass| s.shift.dispatch(pass)),
        ]);
        out
    }

    /// CPU time of writing one CPI's samples into the upload buffer.
    #[cfg(test)]
    pub(crate) fn time_upload(&mut self, job: &CpiJob) -> Result<std::time::Duration, String> {
        let mut encoder = self.encoder();
        let start = std::time::Instant::now();
        self.write_samples(job, &mut encoder)?;
        let elapsed = start.elapsed();
        self.context.queue.submit([encoder.finish()]);
        self.upload.submitted();
        Ok(elapsed)
    }

    /// CPU time of the clutter energies and of the Wiener solve over the sums the last run left.
    #[cfg(test)]
    pub(crate) fn time_clutter(
        &mut self,
        job: &CpiJob,
    ) -> Option<(std::time::Duration, std::time::Duration)> {
        let shape = self.shape;
        let eca = self.eca.as_mut()?;
        let start = std::time::Instant::now();
        eca.solver.solve(&eca.sums, &mut eca.table).ok()?;
        let solve = start.elapsed();
        let start = std::time::Instant::now();
        eca.energies(&shape, job).ok()?;
        Some((start.elapsed(), solve))
    }
}

fn write_complex(context: &Context, target: &wgpu::Buffer, values: &[C32]) -> Result<(), String> {
    let size = wgpu::BufferSize::new(target.size()).ok_or_else(|| "empty GPU upload".to_owned())?;
    let mut view = context
        .queue
        .write_buffer_with(target, 0, size)
        .ok_or_else(|| "GPU upload refused".to_owned())?;
    pack(view.slice(..), std::iter::once(values))
}

fn pack<'a>(
    target: wgpu::WriteOnly<'_, [u8]>,
    parts: impl Iterator<Item = &'a [C32]>,
) -> Result<(), String> {
    let (mut slots, _) = target.into_chunks::<8>();
    for part in parts {
        let mut chunk = slots
            .split_off(..part.len())
            .ok_or_else(|| "GPU upload overflows its buffer".to_owned())?;
        chunk.copy_from_slice(bytemuck::cast_slice(part));
    }
    if slots.is_empty() {
        Ok(())
    } else {
        Err("GPU upload is short".to_owned())
    }
}

fn guarded<T>(
    device: &wgpu::Device,
    work: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let result = work();
    let caught = [internal, memory, validation]
        .map(|scope| pollster::block_on(scope.pop()))
        .into_iter()
        .flatten()
        .next();
    match caught {
        Some(error) => Err(described(&error)),
        None => result,
    }
}

fn described(error: &wgpu::Error) -> String {
    let mut text = format!("GPU error: {error}");
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

fn layout_of(shape: &BatchShape, groups: Option<&GroupPlan>) -> Result<Layout, String> {
    let narrow = |value: usize| {
        u32::try_from(value).map_err(|_| format!("CAF size {value} exceeds 32 bits"))
    };
    let (group_count, shifts, taps) = groups.map_or((0, 0, 0), |plan| {
        (plan.groups(), plan.shifts(), plan.taps())
    });
    Ok(Layout {
        fft_len: narrow(shape.fft_len)?,
        batches: narrow(shape.batches)?,
        batch_len: narrow(shape.batch_len)?,
        gates: narrow(shape.gates)?,
        lead: narrow(shape.lead)?,
        span: narrow(shape.span())?,
        order: narrow(shape.order())?,
        pre: narrow(shape.pre())?,
        window: narrow(shape.window())?,
        lanes: narrow(shape.lanes)?,
        first_lane: 1 + u32::from(shape.eca()),
        slots: narrow(slots(shape))?,
        groups: narrow(group_count)?,
        shifts: narrow(shifts)?,
        taps: narrow(taps)?,
        vectors: narrow(shifts + shape.lanes * taps)?,
        eca: u32::from(groups.is_some()),
        scale: 1.0 / shape.fft_len as f32,
    })
}

fn turns_of(
    solver: &WienerSolver,
    shape: &BatchShape,
    groups: &GroupPlan,
) -> Result<Vec<[f32; 2]>, String> {
    let per_batch = groups.shifts() + groups.taps();
    let mut turns = vec![C32::default(); per_batch];
    let mut out = Vec::with_capacity(groups.groups() * shape.batches * per_batch);
    for group in 0..groups.groups() {
        for batch in 0..shape.batches {
            solver
                .turns(group, batch, &mut turns)
                .map_err(|error| format!("clutter turns: {error}"))?;
            out.extend(turns.iter().map(|turn| [turn.re, turn.im]));
        }
    }
    Ok(out)
}

fn ranges_of(solver: &WienerSolver, groups: &GroupPlan) -> Result<Vec<[u32; 2]>, String> {
    (0..groups.groups())
        .map(|group| {
            let range = solver
                .estimation(group)
                .map_err(|error| format!("clutter groups: {error}"))?;
            let narrow =
                |value: usize| u32::try_from(value).map_err(|_| "group overflow".to_owned());
            Ok([narrow(range.start)?, narrow(range.end)?])
        })
        .collect()
}

fn terms_of(
    table: &WeightTable,
    shape: &BatchShape,
    groups: &GroupPlan,
) -> Result<Vec<Term>, String> {
    let mut mix = vec![MixTerm::default(); 2 * groups.taps()];
    let mut out = Vec::with_capacity(shape.batches * mix.len());
    for batch in 0..shape.batches {
        table
            .terms(batch, &mut mix)
            .map_err(|error| format!("weight mix: {error}"))?;
        for term in &mix {
            out.push(Term {
                group: u32::try_from(term.group).map_err(|_| "group overflow".to_owned())?,
                tap: u32::try_from(term.tap).map_err(|_| "tap overflow".to_owned())?,
                factor: [term.factor.re, term.factor.im],
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;

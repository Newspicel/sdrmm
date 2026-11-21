use super::{
    Context, buffer, bytes_for,
    compute::{Kernel, compile, grid, initialized, module},
};

mod shader;

pub(crate) const MIN_SIZE: usize = 16;
pub(crate) const MAX_SIZE: usize = 1 << 22;
const MAX_TILE: usize = 4096;
const MAX_THREADS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Transforms {
    pub(crate) size: usize,
    pub(crate) offset: usize,
    pub(crate) batches: usize,
    pub(crate) inverse: bool,
}

/// Where twiddle factors come from: `cos`/`sin` on the fly, or a table of the transform size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Twiddles {
    Computed,
    Table,
}

/// Tuning knobs. `span` is the complex values one workgroup holds in shared memory,
/// `columns` the most strided columns sharing it once a transform outgrows it; more columns
/// read memory in longer runs but leave more global passes, so the fewest passes win.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Options {
    pub(crate) radix: usize,
    pub(crate) span: usize,
    pub(crate) columns: usize,
    pub(crate) twiddles: Twiddles,
}

impl Options {
    pub(crate) fn fitting(limits: &wgpu::Limits) -> Self {
        let shared = limits.max_compute_workgroup_storage_size as usize / size_of::<[f32; 2]>();
        let threads = MAX_THREADS.min(limits.max_compute_invocations_per_workgroup as usize);
        let span = prev_power_of_two(shared)
            .min(MAX_TILE)
            .min(threads * 16)
            .max(MIN_SIZE);
        Self {
            radix: 16,
            span,
            columns: 4,
            twiddles: Twiddles::Computed,
        }
    }
}

/// How one size splits into a shared-memory kernel over `columns` columns of `tile` points,
/// then global radix passes. In place, the pass count is odd so the result lands back in
/// `data`; a [`Loader`] reads other buffers, so any count ends there.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Layout {
    tile: usize,
    columns: usize,
    radices: Vec<usize>,
    threads: usize,
    groups: usize,
    passes: Vec<usize>,
}

impl Layout {
    fn new(size: usize, batches: usize, options: Options, in_place: bool) -> Result<Self, String> {
        if !matches!(options.radix, 4 | 8 | 16) || !options.span.is_power_of_two() {
            return Err(format!("unsupported GPU FFT options {options:?}"));
        }
        let radix_bits = options.radix.trailing_zeros();
        let (tile, columns, groups, passes) = if size <= options.span {
            let columns = options.span / size;
            (size, columns, batches.div_ceil(columns), Vec::new())
        } else {
            let most = options.columns.clamp(1, options.span / MIN_SIZE);
            let (columns, passes) = std::iter::successors(Some(1), |c| Some(c * 2))
                .take_while(|&columns| columns <= most)
                .map(|columns| {
                    let outer = (size * columns / options.span).trailing_zeros();
                    (columns, split(outer, radix_bits, in_place))
                })
                .min_by_key(|(columns, passes)| (passes.len(), usize::MAX - columns))
                .unwrap_or((1, Vec::new()));
            let tile = options.span / columns;
            (tile, columns, batches * (size / options.span), passes)
        };
        let radices = split(tile.trailing_zeros(), radix_bits, false);
        let widest = radices.iter().copied().max().unwrap_or(1);
        let elements = widest.max(tile * columns / MAX_THREADS);
        Ok(Self {
            tile,
            columns,
            threads: tile * columns / elements,
            radices,
            groups,
            passes,
        })
    }

    const fn strided(&self) -> bool {
        !self.passes.is_empty()
    }

    const fn elements(&self) -> usize {
        self.tile * self.columns / self.threads
    }
}

/// Splits `bits` into passes of at most `most` bits each, as evenly as possible.
fn split(bits: u32, most: u32, odd: bool) -> Vec<usize> {
    let mut count = bits.div_ceil(most).max(1);
    if odd && count.is_multiple_of(2) {
        count += 1;
    }
    (0..count)
        .map(|pass| 1 << (bits / count + u32::from(pass < bits % count)))
        .collect()
}

const fn prev_power_of_two(value: usize) -> usize {
    if value == 0 {
        0
    } else {
        1 << (usize::BITS - 1 - value.leading_zeros())
    }
}

/// Feeds the first stage from the caller's own buffers instead of `data`, saving the pass
/// that would stage samples there. `wgsl` defines `fn sample(transform: u32, index: u32) ->
/// vec2<f32>` over bindings from 3 up; each entry of `bindings` binds one set of them, and
/// [`FftBatch::dispatch_from`] picks the set. An `in_place` loader reads `data` itself, each
/// transform only from its own span or outside the transformed range.
pub(crate) struct Loader<'a> {
    pub(crate) wgsl: &'a str,
    pub(crate) bindings: Vec<Vec<(u32, &'a wgpu::Buffer)>>,
    pub(crate) in_place: bool,
}

/// Batched Stockham FFTs over natural-order complex data, in place in `data` or loaded
/// into it through a [`Loader`].
pub(crate) struct FftBatch {
    first: Vec<Kernel>,
    passes: Vec<Kernel>,
}

struct Buffers<'a> {
    data: &'a wgpu::Buffer,
    scratch: wgpu::Buffer,
    table: wgpu::Buffer,
}

impl FftBatch {
    #[cfg(test)]
    pub(crate) fn new(
        context: &Context,
        data: &wgpu::Buffer,
        size: usize,
        batches: usize,
        inverse: bool,
    ) -> Result<Self, String> {
        Self::over(
            context,
            data,
            Transforms {
                size,
                offset: 0,
                batches,
                inverse,
            },
        )
    }

    pub(crate) fn over(
        context: &Context,
        data: &wgpu::Buffer,
        plan: Transforms,
    ) -> Result<Self, String> {
        let options = Options::fitting(&context.device.limits());
        Self::with_options(context, data, plan, options)
    }

    pub(crate) fn with_options(
        context: &Context,
        data: &wgpu::Buffer,
        plan: Transforms,
        options: Options,
    ) -> Result<Self, String> {
        Self::build(context, data, plan, options, None)
    }

    pub(crate) fn loaded(
        context: &Context,
        data: &wgpu::Buffer,
        plan: Transforms,
        loader: &Loader<'_>,
    ) -> Result<Self, String> {
        let options = Options::fitting(&context.device.limits());
        Self::build(context, data, plan, options, Some(loader))
    }

    fn build(
        context: &Context,
        data: &wgpu::Buffer,
        plan: Transforms,
        options: Options,
        loader: Option<&Loader<'_>>,
    ) -> Result<Self, String> {
        check_size(plan.size)?;
        let batches = plan.batches.max(1);
        let elements = plan
            .size
            .checked_mul(batches)
            .ok_or_else(|| "GPU FFT batch overflow".to_owned())?;
        u32::try_from(plan.offset.saturating_add(elements))
            .map_err(|_| format!("GPU FFT index {} exceeds 32 bits", plan.offset + elements))?;
        let in_place = loader.is_none_or(|loader| loader.in_place);
        let layout = Layout::new(plan.size, batches, options, in_place)?;
        let shape = shader::Shape {
            size: plan.size,
            batches,
            offset: plan.offset,
            inverse: plan.inverse,
            loaded: loader.is_some(),
            in_place,
        };
        let wgsl = loader.map(|loader| loader.wgsl);
        let (source, entries) = shader::generate(&layout, shape, options.twiddles, wgsl);
        let module = module(context, &source, "fft");
        let scratch_bytes = if layout.strided() {
            bytes_for::<[f32; 2]>(elements)?
        } else {
            8
        };
        let buffers = Buffers {
            data,
            scratch: buffer(
                &context.device,
                "SDRmm FFT scratch",
                scratch_bytes,
                wgpu::BufferUsages::STORAGE,
            ),
            table: initialized(context, &table(plan.size, options.twiddles)),
        };
        let (head, tail) = entries
            .split_first()
            .ok_or_else(|| "GPU FFT has no kernels".to_owned())?;
        let first_groups = grid(layout.groups as u64, 1)?;
        let pipeline = compile(context, &module, &head.name);
        let first = match loader {
            Some(loader) => loader
                .bindings
                .iter()
                .map(|set| bind(context, &pipeline, head, &buffers, set, first_groups))
                .collect(),
            None => vec![bind(context, &pipeline, head, &buffers, &[], first_groups)],
        };
        let threads = shader::global_threads() as u64;
        let passes = tail
            .iter()
            .zip(&layout.passes)
            .map(|(entry, radix)| {
                let pipeline = compile(context, &module, &entry.name);
                let groups = grid((elements / radix) as u64, threads)?;
                Ok(bind(context, &pipeline, entry, &buffers, &[], groups))
            })
            .collect::<Result<_, String>>()?;
        Ok(Self { first, passes })
    }

    #[cfg(test)]
    pub(crate) fn kernels(&self) -> impl Iterator<Item = &Kernel> {
        self.first.iter().take(1).chain(&self.passes)
    }

    pub(crate) fn dispatch(&self, pass: &mut wgpu::ComputePass<'_>) {
        self.dispatch_from(pass, 0);
    }

    /// Runs the transforms, loading through binding set `input` of the [`Loader`].
    pub(crate) fn dispatch_from(&self, pass: &mut wgpu::ComputePass<'_>, input: usize) {
        if let Some(first) = self.first.get(input).or_else(|| self.first.first()) {
            first.dispatch(pass);
        }
        for kernel in &self.passes {
            kernel.dispatch(pass);
        }
    }
}

fn bind(
    context: &Context,
    pipeline: &wgpu::ComputePipeline,
    entry: &shader::Entry,
    buffers: &Buffers<'_>,
    loads: &[(u32, &wgpu::Buffer)],
    groups: [u32; 3],
) -> Kernel {
    let mut bound = Vec::with_capacity(3 + loads.len());
    if entry.data {
        bound.push((0, buffers.data));
    }
    if entry.scratch {
        bound.push((1, &buffers.scratch));
    }
    if entry.table {
        bound.push((2, &buffers.table));
    }
    if entry.loads {
        bound.extend_from_slice(loads);
    }
    Kernel::bind(context, pipeline, &bound, groups)
}

pub(crate) fn check_size(size: usize) -> Result<(), String> {
    if size.is_power_of_two() && (MIN_SIZE..=MAX_SIZE).contains(&size) {
        Ok(())
    } else {
        Err(format!(
            "GPU FFT size {size} is not a power of two from {MIN_SIZE} to {MAX_SIZE}"
        ))
    }
}

fn table(size: usize, twiddles: Twiddles) -> Vec<[f32; 2]> {
    let len = if twiddles == Twiddles::Table { size } else { 1 };
    (0..len)
        .map(|index| {
            let angle = -std::f64::consts::TAU * index as f64 / size as f64;
            [angle.cos() as f32, angle.sin() as f32]
        })
        .collect()
}

#[cfg(test)]
mod tests;

use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

use bytemuck::Pod;
use wgpu::util::DeviceExt;

use super::{Context, bind, buffer};

const MAX_GROUPS_PER_DIMENSION: u64 = 65_535;
const PATIENCE: Duration = Duration::from_secs(2);
pub(crate) const PENDING: u8 = 0;
pub(crate) const MAPPED: u8 = 1;
pub(crate) const FAILED: u8 = 2;
const WRITTEN: u8 = 3;

/// A named piece of GPU work, for profiling it on its own.
#[cfg(test)]
pub(crate) type Stage<'a> = (&'static str, Box<dyn Fn(&mut wgpu::ComputePass<'_>) + 'a>);

pub(crate) struct Kernel {
    pipeline: wgpu::ComputePipeline,
    bindings: wgpu::BindGroup,
    groups: [u32; 3],
}

impl Kernel {
    #[cfg(test)]
    pub(crate) fn new(
        context: &Context,
        source: &str,
        entry: &str,
        buffers: &[&wgpu::Buffer],
        groups: [u32; 3],
    ) -> Self {
        let pipeline = compile(context, &module(context, source, entry), entry);
        let bindings: Vec<_> = (0u32..).zip(buffers.iter().copied()).collect();
        Self::bind(context, &pipeline, &bindings, groups)
    }

    pub(crate) fn bind(
        context: &Context,
        pipeline: &wgpu::ComputePipeline,
        buffers: &[(u32, &wgpu::Buffer)],
        groups: [u32; 3],
    ) -> Self {
        let entries: Vec<_> = buffers
            .iter()
            .map(|(index, buffer)| bind(*index, buffer.as_entire_binding()))
            .collect();
        let bindings = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &entries,
            });
        Self {
            pipeline: pipeline.clone(),
            bindings,
            groups,
        }
    }

    pub(crate) fn dispatch(&self, pass: &mut wgpu::ComputePass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bindings, &[]);
        pass.dispatch_workgroups(self.groups[0], self.groups[1], self.groups[2]);
    }
}

pub(crate) fn module(context: &Context, source: &str, label: &str) -> wgpu::ShaderModule {
    context
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        })
}

/// Leaves workgroup memory uninitialized: naga zeroes it from one thread per workgroup,
/// which made a 32 KiB FFT tile 70x slower. Every kernel writes shared memory before reading it.
pub(crate) fn compile(
    context: &Context,
    module: &wgpu::ShaderModule,
    entry: &str,
) -> wgpu::ComputePipeline {
    context
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: None,
            module,
            entry_point: Some(entry),
            compilation_options: wgpu::PipelineCompilationOptions {
                zero_initialize_workgroup_memory: false,
                ..Default::default()
            },
            cache: None,
        })
}

pub(crate) fn grid(threads: u64, per_group: u64) -> Result<[u32; 3], String> {
    let groups = threads.div_ceil(per_group.max(1)).max(1);
    let x = groups.min(MAX_GROUPS_PER_DIMENSION);
    let y = groups.div_ceil(x);
    if y > MAX_GROUPS_PER_DIMENSION {
        return Err(format!("{threads} GPU threads exceed the dispatch limit"));
    }
    let narrow = |value: u64| u32::try_from(value).map_err(|_| "GPU dispatch overflow".to_owned());
    Ok([narrow(x)?, narrow(y)?, 1])
}

pub(crate) fn initialized<T: Pod>(context: &Context, contents: &[T]) -> wgpu::Buffer {
    context
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("SDRmm data"),
            contents: bytemuck::cast_slice(contents),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        })
}

pub(crate) struct Readback {
    buffer: wgpu::Buffer,
    bytes: u64,
    state: Arc<AtomicU8>,
}

impl Readback {
    pub(crate) fn new(context: &Context, label: &str, bytes: u64) -> Self {
        Self {
            buffer: buffer(
                &context.device,
                label,
                bytes,
                wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            ),
            bytes,
            state: Arc::new(AtomicU8::new(PENDING)),
        }
    }

    pub(crate) fn copy(&self, encoder: &mut wgpu::CommandEncoder, source: &wgpu::Buffer) {
        encoder.copy_buffer_to_buffer(source, 0, &self.buffer, 0, self.bytes);
    }

    pub(crate) fn wait(
        &self,
        context: &Context,
        submission: wgpu::SubmissionIndex,
    ) -> Result<(), String> {
        self.state.store(PENDING, Ordering::Release);
        let state = Arc::clone(&self.state);
        self.buffer
            .map_async(wgpu::MapMode::Read, .., move |result| {
                let outcome = if result.is_ok() { MAPPED } else { FAILED };
                state.store(outcome, Ordering::Release);
            });
        context
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(PATIENCE),
            })
            .map_err(|error| format!("wait for the GPU: {error}"))?;
        settle(context, &self.state)
    }

    pub(crate) fn read<T: Pod>(
        &self,
        into: impl FnOnce(&[T]) -> Result<(), String>,
    ) -> Result<(), String> {
        let result = self
            .buffer
            .get_mapped_range(..)
            .map_err(|error| format!("read GPU output: {error}"))
            .and_then(|view| {
                let values = bytemuck::try_cast_slice(&view)
                    .map_err(|error| format!("GPU output layout: {error}"))?;
                into(values)
            });
        self.buffer.unmap();
        result
    }
}

/// How samples reach the GPU: through a staging copy, or written in place into buffers the
/// kernels read, where the GPU shares memory with the CPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Transfer {
    Staged,
    Direct,
}

impl Transfer {
    pub(crate) const fn fitting(context: &Context) -> Self {
        if context.unified() {
            Self::Direct
        } else {
            Self::Staged
        }
    }
}

/// A ring of persistently mapped buffers, so an upload costs one memcpy instead of a fresh
/// wgpu staging allocation per write. Staged, each is copied into one storage buffer;
/// direct, kernels read the ring buffers themselves.
pub(crate) struct Upload {
    slots: Vec<(wgpu::Buffer, Arc<AtomicU8>)>,
    target: Option<wgpu::Buffer>,
    next: usize,
}

impl Upload {
    pub(crate) fn new(
        context: &Context,
        label: &str,
        bytes: u64,
        slots: usize,
        transfer: Transfer,
    ) -> Self {
        let usage = match transfer {
            Transfer::Staged => wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
            Transfer::Direct => wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::STORAGE,
        };
        let slots = (0..slots.max(1))
            .map(|_| {
                let buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size: bytes,
                    usage,
                    mapped_at_creation: true,
                });
                (buffer, Arc::new(AtomicU8::new(MAPPED)))
            })
            .collect();
        let target = (transfer == Transfer::Staged).then(|| {
            buffer(
                &context.device,
                label,
                bytes,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            )
        });
        Self {
            slots,
            target,
            next: 0,
        }
    }

    /// The buffers kernels read; [`Self::write`] says which one holds the latest upload.
    pub(crate) fn inputs(&self) -> Vec<&wgpu::Buffer> {
        match &self.target {
            Some(target) => vec![target],
            None => self.slots.iter().map(|(buffer, _)| buffer).collect(),
        }
    }

    /// Writes `parts` back to back into the next ring buffer and returns the index of the
    /// input that holds them. Call [`Self::submitted`] once the encoder is submitted.
    pub(crate) fn write(
        &self,
        context: &Context,
        encoder: &mut wgpu::CommandEncoder,
        parts: &[&[u8]],
    ) -> Result<usize, String> {
        let (buffer, state) = &self.slots[self.next];
        let len: u64 = parts.iter().map(|part| part.len() as u64).sum();
        if len > buffer.size() {
            return Err(format!("{len} upload bytes exceed the upload buffer"));
        }
        rearm(buffer, state);
        settle(context, state)?;
        let mut at = 0;
        let filled = parts
            .iter()
            .filter(|part| !part.is_empty())
            .try_for_each(|part| {
                let end = at + part.len() as u64;
                let mut view = buffer
                    .get_mapped_range_mut(at..end)
                    .map_err(|error| format!("write GPU input: {error}"))?;
                view.copy_from_slice(part);
                at = end;
                Ok::<_, String>(())
            });
        buffer.unmap();
        state.store(WRITTEN, Ordering::Release);
        filled?;
        match &self.target {
            Some(target) => {
                encoder.copy_buffer_to_buffer(buffer, 0, target, 0, len);
                Ok(0)
            }
            None => Ok(self.next),
        }
    }

    /// Maps the buffer just written again for when the GPU is done with it, and moves on to
    /// the next one. Without a write since the last call it does nothing.
    pub(crate) fn submitted(&mut self) {
        let (buffer, state) = &self.slots[self.next];
        if state.load(Ordering::Acquire) != WRITTEN {
            return;
        }
        rearm(buffer, state);
        self.next = (self.next + 1) % self.slots.len();
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.slots.first().map_or(0, |(buffer, _)| buffer.size())
    }
}

/// Maps a buffer written since its last mapping, so a write never meets an unmapped buffer
/// even when an error skipped [`Upload::submitted`].
fn rearm(buffer: &wgpu::Buffer, state: &Arc<AtomicU8>) {
    if state.load(Ordering::Acquire) != WRITTEN {
        return;
    }
    state.store(PENDING, Ordering::Release);
    let state = Arc::clone(state);
    buffer.map_async(wgpu::MapMode::Write, .., move |result| {
        let outcome = if result.is_ok() { MAPPED } else { FAILED };
        state.store(outcome, Ordering::Release);
    });
}

pub(crate) fn settle(context: &Context, state: &AtomicU8) -> Result<(), String> {
    let deadline = Instant::now() + PATIENCE;
    loop {
        match state.load(Ordering::Acquire) {
            MAPPED => return Ok(()),
            FAILED => return Err("GPU mapping failed".to_owned()),
            _ if Instant::now() >= deadline => return Err("GPU mapping timed out".to_owned()),
            _ => {
                context
                    .device
                    .poll(wgpu::PollType::Poll)
                    .map_err(|error| format!("poll the GPU: {error}"))?;
                std::thread::yield_now();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::grid;

    #[test]
    fn a_grid_covers_every_thread_within_the_dimension_limit() {
        assert_eq!(grid(0, 256), Ok([1, 1, 1]));
        assert_eq!(grid(256 * 1000, 256), Ok([1000, 1, 1]));
        let threads = 256 * 70_000 + 1;
        let [x, y, _] = grid(threads, 256).unwrap();
        assert_eq!(x, 65_535);
        assert!(u64::from(x) * u64::from(y) * 256 >= threads);
        assert!(grid(u64::MAX / 2, 1).is_err());
    }
}

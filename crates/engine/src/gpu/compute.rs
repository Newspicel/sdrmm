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
            compilation_options: Default::default(),
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

use std::sync::{Arc, OnceLock};

pub(crate) fn context() -> Result<&'static Arc<Context>, &'static str> {
    static CONTEXT: OnceLock<Result<Arc<Context>, String>> = OnceLock::new();
    CONTEXT
        .get_or_init(|| Context::new().map(Arc::new))
        .as_ref()
        .map_err(String::as_str)
}

pub(crate) struct Context {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_name: String,
}

impl Context {
    pub(crate) fn new() -> Result<Self, String> {
        Self::new_inner(false)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test() -> Result<Self, String> {
        Self::new_inner(true)
    }

    fn new_inner(allow_cpu_adapter: bool) -> Result<Self, String> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|error| format!("request adapter: {error}"))?;
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu && !allow_cpu_adapter {
            return Err(format!(
                "adapter '{}' is software-rendered and would not accelerate FFTs",
                info.name
            ));
        }
        tracing::info!(adapter = %info.name, backend = ?info.backend, "GPU compute adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("SDRmm GPU compute"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|error| format!("request device: {error}"))?;

        Ok(Self {
            device,
            queue,
            adapter_name: info.name,
        })
    }

    pub(crate) fn adapter_name(&self) -> &str {
        &self.adapter_name
    }
}

fn buffer(
    device: &wgpu::Device,
    label: &str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage,
        mapped_at_creation: false,
    })
}

fn bind(binding: u32, resource: wgpu::BindingResource<'_>) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource }
}

fn bytes_for<T>(len: usize) -> Result<u64, String> {
    len.checked_mul(size_of::<T>())
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or_else(|| "FFT buffer size overflow".to_string())
}

#[cfg(test)]
pub(crate) mod benchmarks;

pub(crate) mod caf;
mod compute;
mod fft;
pub(crate) mod spectrum;

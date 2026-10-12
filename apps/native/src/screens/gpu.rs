use std::sync::{Arc, Mutex};

use zgui::{
    platform::Surface,
    render::{
        ExternalTexture, FrameOutcome, GpuUnavailable, MemoryReport, RenderCapabilities,
        RenderTarget, Renderer, ScrollShift, TargetPoolReport, TextureHandle,
    },
    runtime::{AppError, RendererFactory},
};
use zgui_render_wgpu::{Builder, Pixels, WgpuRenderer, wgpu};

static CREATING: Mutex<()> = Mutex::new(());

pub struct Offscreen(WgpuRenderer);

impl Offscreen {
    pub fn read(&self) -> Pixels {
        self.0.read_composed()
    }
}

fn offscreen(target: RenderTarget) -> Result<WgpuRenderer, GpuUnavailable> {
    let _creating = CREATING.lock().unwrap_or_else(|held| held.into_inner());
    Builder::new().offscreen(target, wgpu::TextureFormat::Bgra8Unorm, false)
}

fn open(target: RenderTarget) -> Result<Box<dyn Renderer>, AppError> {
    let mut renderer = offscreen(target)?;
    renderer.set_vector_shortfall_fatal(false);
    zgui_render_vector_vello::attach(&mut renderer, target.size);
    Ok(Box::new(Offscreen(renderer)))
}

pub fn factory() -> Option<RendererFactory> {
    let probe = RenderTarget::new(zgui::geom::Size::new(64, 64), zgui::geom::Scale::new(1.0));
    offscreen(probe).ok()?;
    Some(Box::new(|_: &Arc<dyn Surface>, target: RenderTarget| {
        open(target)
    }))
}

impl Renderer for Offscreen {
    fn capabilities(&self) -> RenderCapabilities {
        self.0.capabilities()
    }

    fn configure(&mut self, target: RenderTarget) {
        Renderer::configure(&mut self.0, target);
    }

    fn target(&self) -> Option<RenderTarget> {
        self.0.target()
    }

    fn draw(&mut self, scene: &zgui::scene::Scene, damage: &zgui::bits::DamageSet) -> FrameOutcome {
        self.0.draw(scene, damage)
    }

    fn shifts_composed_pixels(&self) -> bool {
        self.0.shifts_composed_pixels()
    }

    fn composed_survives_configure(&self) -> bool {
        self.0.composed_survives_configure()
    }

    fn shift_composed(&mut self, shift: ScrollShift) {
        self.0.shift_composed(shift);
    }

    fn register_external(&mut self, texture: ExternalTexture) -> TextureHandle {
        self.0.register_external(texture)
    }

    fn release_external(&mut self, handle: TextureHandle) {
        self.0.release_external(handle);
    }

    fn memory(&self) -> MemoryReport {
        self.0.memory()
    }

    fn vector_status(&self) -> zgui::render::VectorStatus {
        self.0.vector_status()
    }

    fn target_pool(&self) -> TargetPoolReport {
        self.0.target_pool()
    }

    fn release_cached_targets(&mut self) -> u64 {
        self.0.release_cached_targets()
    }

    fn release_idle_resources(&mut self) -> u64 {
        self.0.release_idle_resources()
    }

    fn acquire_block(&self) -> std::time::Duration {
        self.0.acquire_block()
    }

    fn texture_sink(&mut self) -> &mut dyn zgui::atlas::TextureSink {
        self.0.texture_sink()
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        self.0.as_any_mut()
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

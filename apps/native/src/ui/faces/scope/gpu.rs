use std::{
    cell::RefCell,
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

use zgui::{
    prelude::*,
    surface::{SurfaceElementExt, SurfaceRenderCx, SurfaceRenderer, wgpu},
};

use super::{
    clock::RowClock,
    colormap::colormap_wgsl,
    history::{HISTORY_ROWS, next_ring_row, rows_for_height, seed_placement},
    palette::{CUSTOM_INDEX, CUSTOM_STOPS, Palette},
};

pub use super::history::RowLevel;

const FIRST_BINS: usize = 1024;
const ROW_FIELDS: usize = 4;
const PARAMS_HEAD: usize = 32;
const PARAMS_BYTES: u64 = (PARAMS_HEAD + CUSTOM_STOPS * 16) as u64;

pub const VERTEX_WGSL: &str = r"
struct Varying {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> Varying {
    var out: Varying;
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    out.uv = vec2<f32>(x, y);
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    return out;
}
";

const WATERFALL_WGSL: &str = r"
struct Params {
    write: f32,
    height: f32,
    rows: f32,
    view_start: f32,
    view_width: f32,
    pixels: f32,
    map: u32,
    spare: u32,
    custom: array<vec4<f32>, 5>,
}

@group(0) @binding(0) var history: texture_2d<f32>;
@group(0) @binding(1) var linear: sampler;
@group(0) @binding(2) var levels: texture_2d<f32>;
@group(0) @binding(3) var<uniform> params: Params;

const MAX_TAPS: i32 = 32;
const DB_TO_LOG2: f32 = 0.33219281;

fn wrap(value: f32, size: f32) -> f32 {
    return value - size * floor(value / size);
}

fn footprint(tx: f32, row: i32, level: vec4<f32>) -> f32 {
    let bins = i32(textureDimensions(history).x);
    let left = tx * f32(bins);
    let right = left + params.view_width * f32(bins) / max(params.pixels, 1.0);
    let first = i32(floor(left));
    let last = max(first, i32(ceil(right)) - 1);
    let scale = level.b * DB_TO_LOG2;
    var peak = 0.0;
    var power = 0.0;
    var taps = 0.0;
    for (var i = 0; i < MAX_TAPS; i++) {
        let x = first + i;
        if (x > last) {
            break;
        }
        if (x >= 0 && x < bins) {
            let v = textureLoad(history, vec2<i32>(x, row), 0).r;
            peak = max(peak, v);
            power += exp2(v * scale);
            taps += 1.0;
        }
    }
    if (peak >= level.g || scale <= 0.0 || taps == 0.0) {
        return peak;
    }
    return log2(power / taps) / scale;
}

fn custom(t: f32) -> vec3<f32> {
    let x = t * 4.0;
    let i = min(i32(floor(x)), 3);
    return mix(params.custom[i].rgb, params.custom[i + 1].rgb, x - f32(i));
}

fn palette(level: f32) -> vec3<f32> {
    if (params.map == CUSTOM_MAP) {
        return custom(clamp(level, 0.0, 1.0));
    }
    return colormap(level, params.map);
}

@fragment
fn fragment(in: Varying) -> @location(0) vec4<f32> {
    let rows_back = in.uv.y * (params.rows - 1.0);
    let row = wrap(params.write - 1.0 - rows_back, params.height);
    let ty = (row + 0.5) / params.height;
    let level = textureLoad(levels, vec2<i32>(0, i32(row)), 0);
    let tx = params.view_start + in.uv.x * params.view_width + level.r;
    let bins = f32(textureDimensions(history).x);
    let per_pixel = params.view_width * bins / max(params.pixels, 1.0);
    var v = 0.0;
    if (tx >= 0.0 && tx <= 1.0) {
        if (per_pixel > 1.0) {
            v = footprint(tx, i32(row), level);
        } else {
            v = textureSampleLevel(history, linear, vec2<f32>(tx, ty), 0.0).r;
        }
    }
    return vec4<f32>(palette(v), 1.0);
}
";

pub enum Pending {
    Row(Vec<u8>, RowLevel),
    Seed {
        rows: Vec<u8>,
        count: usize,
        bins: usize,
        levels: Vec<RowLevel>,
    },
    Shift(f64),
}

pub struct WaterfallFeed {
    pending: Vec<Pending>,
    pub start: f64,
    pub width: f64,
    pub palette: Palette,
    pub clock: RowClock,
    changed: bool,
}

impl Default for WaterfallFeed {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            start: 0.0,
            width: 1.0,
            palette: Palette::default(),
            clock: RowClock::default(),
            changed: true,
        }
    }
}

impl WaterfallFeed {
    pub fn push_row(&mut self, bins: &[u8], level: RowLevel) {
        if !bins.is_empty() {
            self.clock.push(level.at_ms, bins.len());
            self.pending.push(Pending::Row(bins.to_vec(), level));
        }
    }

    pub fn seed(&mut self, rows: Vec<u8>, count: usize, bins: usize, levels: Vec<RowLevel>) {
        if bins == 0 {
            return;
        }
        let times: Vec<f64> = levels.iter().map(|level| level.at_ms).collect();
        self.clock.seed(&times, count, bins);
        self.pending
            .retain(|pending| matches!(pending, Pending::Shift(_)));
        self.pending.push(Pending::Seed {
            rows,
            count,
            bins,
            levels,
        });
    }

    pub fn shift_rows(&mut self, delta: f64) {
        if delta != 0.0 {
            self.pending.push(Pending::Shift(delta));
        }
    }

    pub fn set_window(&mut self, start: f64, width: f64) {
        self.changed |= self.start != start || self.width != width;
        self.start = start;
        self.width = width;
    }

    pub fn set_palette(&mut self, palette: Palette) {
        self.changed |= self.palette != palette;
        self.palette = palette;
    }
}

pub struct Ring {
    pub bins: usize,
    pub write: usize,
    pub rows: Vec<[f32; ROW_FIELDS]>,
    pub rows_dirty: bool,
}

impl Default for Ring {
    fn default() -> Self {
        Self {
            bins: FIRST_BINS,
            write: 0,
            rows: vec![[0.0; ROW_FIELDS]; HISTORY_ROWS],
            rows_dirty: true,
        }
    }
}

fn row_fields(shift: f32, level: RowLevel) -> [f32; ROW_FIELDS] {
    [shift, level.signal, level.span_db, 0.0]
}

pub enum Upload<'a> {
    Reallocate(usize),
    Rows {
        at: usize,
        rows: &'a [u8],
        count: usize,
    },
}

impl Ring {
    pub fn apply<'a>(&mut self, pending: &'a Pending, mut upload: impl FnMut(Upload<'a>)) {
        match pending {
            Pending::Row(bins, level) => {
                if bins.len() != self.bins {
                    self.allocate(bins.len());
                    upload(Upload::Reallocate(self.bins));
                }
                self.rows[self.write] = row_fields(0.0, *level);
                self.rows_dirty = true;
                upload(Upload::Rows {
                    at: self.write,
                    rows: bins,
                    count: 1,
                });
                self.write = next_ring_row(self.write, HISTORY_ROWS);
            }
            Pending::Seed {
                rows,
                count,
                bins,
                levels,
            } => {
                let place = seed_placement(*count, HISTORY_ROWS);
                if place.rows == 0 {
                    return;
                }
                self.allocate(*bins);
                upload(Upload::Reallocate(self.bins));
                for (row, slot) in self.rows.iter_mut().take(place.rows).enumerate() {
                    let level = levels
                        .get(place.skip + row)
                        .copied()
                        .unwrap_or(RowLevel::PLAIN);
                    *slot = row_fields(0.0, level);
                }
                let from = place.skip * bins;
                upload(Upload::Rows {
                    at: 0,
                    rows: &rows[from..from + place.rows * bins],
                    count: place.rows,
                });
                self.write = place.write;
            }
            Pending::Shift(delta) => {
                for row in &mut self.rows {
                    row[0] += *delta as f32;
                }
                self.rows_dirty = true;
            }
        }
    }

    fn allocate(&mut self, bins: usize) {
        self.bins = bins.max(1);
        self.write = 0;
        self.rows.fill([0.0; ROW_FIELDS]);
        self.rows_dirty = true;
    }

    fn row_bytes(&self, out: &mut Vec<u8>) {
        out.clear();
        out.extend(
            self.rows
                .iter()
                .flatten()
                .flat_map(|value| value.to_le_bytes()),
        );
    }
}

struct Resources {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    history: wgpu::Texture,
    levels: wgpu::Texture,
    params: wgpu::Buffer,
    binding: wgpu::BindGroup,
    format: wgpu::TextureFormat,
    device: usize,
}

pub struct WaterfallSurface {
    feed: Rc<RefCell<WaterfallFeed>>,
    failure: RwSignal<Option<String>>,
    ring: Ring,
    held: Option<Resources>,
    drawn: Option<(u32, u32)>,
    staging: Vec<u8>,
}

impl WaterfallSurface {
    pub fn new(feed: Rc<RefCell<WaterfallFeed>>, failure: RwSignal<Option<String>>) -> Self {
        Self {
            feed,
            failure,
            ring: Ring::default(),
            held: None,
            drawn: None,
            staging: Vec::new(),
        }
    }
}

pub fn waterfall_view(
    class: &'static str,
    feed: Rc<RefCell<WaterfallFeed>>,
    failure: RwSignal<Option<String>>,
) -> AnyView {
    AnyView::new(
        zgui::elements::surface()
            .class(class)
            .renderer(WaterfallSurface::new(feed, failure))
            .into_view(),
    )
}

#[must_use]
pub fn settled<F: Future>(future: F) -> Option<F::Output> {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => Some(output),
        Poll::Pending => None,
    }
}

fn device_id(device: &wgpu::Device) -> usize {
    std::ptr::from_ref(device) as usize
}

pub fn texture(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

pub fn texture_entry(binding: u32, filterable: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

pub fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

pub fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

pub fn linear_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("scope.linear"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..wgpu::SamplerDescriptor::default()
    })
}

pub fn uniform_buffer(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub fn pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(format.into())],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

pub fn draw(cx: &SurfaceRenderCx<'_>, pipeline: &wgpu::RenderPipeline, binding: &wgpu::BindGroup) {
    let mut encoder = cx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("scope.draw"),
        });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scope.pass"),
            multiview_mask: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: cx.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, binding, &[]);
        pass.draw(0..3, 0..1);
    }
    cx.queue.submit([encoder.finish()]);
}

pub fn write_rows(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    at: u32,
    bytes: &[u8],
    width: u32,
    rows: u32,
    texel: u32,
) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d { x: 0, y: at, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * texel),
            rows_per_image: Some(rows),
        },
        wgpu::Extent3d {
            width,
            height: rows,
            depth_or_array_layers: 1,
        },
    );
}

#[must_use]
pub fn waterfall_source() -> String {
    format!(
        "{VERTEX_WGSL}\n{}\nconst CUSTOM_MAP: u32 = {CUSTOM_INDEX}u;\n{WATERFALL_WGSL}",
        colormap_wgsl()
    )
}

impl Resources {
    fn build(cx: &SurfaceRenderCx<'_>, bins: usize) -> Self {
        let device = cx.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("waterfall.layout"),
            entries: &[
                texture_entry(0, true),
                sampler_entry(1),
                texture_entry(2, false),
                uniform_entry(3),
            ],
        });
        let format = cx.texture.format();
        let pipeline = pipeline(device, "waterfall", &waterfall_source(), &layout, format);
        let sampler = linear_sampler(device);
        let levels = texture(
            device,
            "waterfall.levels",
            1,
            HISTORY_ROWS as u32,
            wgpu::TextureFormat::Rgba32Float,
        );
        let params = uniform_buffer(device, "waterfall.params", PARAMS_BYTES);
        let history = texture(
            device,
            "waterfall.history",
            bins as u32,
            HISTORY_ROWS as u32,
            wgpu::TextureFormat::R8Unorm,
        );
        let binding = bind(device, &layout, &history, &sampler, &levels, &params);
        Self {
            pipeline,
            layout,
            sampler,
            history,
            levels,
            params,
            binding,
            format,
            device: device_id(device),
        }
    }

    fn reallocate(&mut self, device: &wgpu::Device, bins: usize) {
        self.history = texture(
            device,
            "waterfall.history",
            bins as u32,
            HISTORY_ROWS as u32,
            wgpu::TextureFormat::R8Unorm,
        );
        self.binding = bind(
            device,
            &self.layout,
            &self.history,
            &self.sampler,
            &self.levels,
            &self.params,
        );
    }
}

fn bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    history: &wgpu::Texture,
    sampler: &wgpu::Sampler,
    levels: &wgpu::Texture,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    let view =
        |texture: &wgpu::Texture| texture.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("waterfall.binding"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view(history)),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&view(levels)),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: params.as_entire_binding(),
            },
        ],
    })
}

#[must_use]
pub fn params_bytes(values: [f32; 6], palette: Palette) -> [u8; PARAMS_BYTES as usize] {
    let mut bytes = [0u8; PARAMS_BYTES as usize];
    for (at, value) in values.iter().enumerate() {
        bytes[at * 4..at * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[24..28].copy_from_slice(&palette.shader_index().to_le_bytes());
    for (stop, rgb) in palette.custom_stops().iter().enumerate() {
        for (channel, value) in rgb.iter().enumerate() {
            let at = PARAMS_HEAD + stop * 16 + channel * 4;
            bytes[at..at + 4].copy_from_slice(&(*value as f32).to_le_bytes());
        }
    }
    bytes
}

impl WaterfallSurface {
    fn ready(&mut self, cx: &SurfaceRenderCx<'_>) -> Option<bool> {
        let stale = self.held.as_ref().is_none_or(|held| {
            held.format != cx.texture.format() || held.device != device_id(cx.device)
        });
        if !stale {
            return Some(true);
        }
        if self.failure.with_untracked(Option::is_some) {
            return None;
        }
        self.ring = Ring::default();
        let scope = cx.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let built = Resources::build(cx, self.ring.bins);
        if let Some(error) = settled(scope.pop()).flatten() {
            tracing::warn!(%error, "the waterfall cannot draw");
            self.failure.set(Some(error.to_string()));
            self.held = None;
            return None;
        }
        self.held = Some(built);
        self.drawn = None;
        Some(false)
    }

    fn upload(&mut self, cx: &SurfaceRenderCx<'_>) -> bool {
        let pending = std::mem::take(&mut self.feed.borrow_mut().pending);
        let Some(held) = self.held.as_mut() else {
            return false;
        };
        for entry in &pending {
            self.ring.apply(entry, |upload| match upload {
                Upload::Reallocate(bins) => held.reallocate(cx.device, bins),
                Upload::Rows { at, rows, count } => write_rows(
                    cx.queue,
                    &held.history,
                    at as u32,
                    rows,
                    (rows.len() / count.max(1)) as u32,
                    count as u32,
                    1,
                ),
            });
        }
        if self.ring.rows_dirty {
            self.ring.row_bytes(&mut self.staging);
            write_rows(
                cx.queue,
                &held.levels,
                0,
                &self.staging,
                1,
                HISTORY_ROWS as u32,
                (ROW_FIELDS * 4) as u32,
            );
            self.ring.rows_dirty = false;
        }
        !pending.is_empty()
    }
}

impl SurfaceRenderer for WaterfallSurface {
    fn render(&mut self, cx: &mut SurfaceRenderCx<'_>) {
        cx.request_animation_frame();
        if cx.size.width == 0 || cx.size.height == 0 {
            return;
        }
        let Some(ready) = self.ready(cx) else {
            return;
        };
        let fresh = !ready;
        let uploaded = self.upload(cx);
        let size = (cx.size.width, cx.size.height);
        let (start, width, palette, changed) = {
            let mut feed = self.feed.borrow_mut();
            let changed = std::mem::take(&mut feed.changed);
            (feed.start, feed.width, feed.palette, changed)
        };
        if !(fresh || uploaded || changed || self.drawn != Some(size)) {
            return;
        }
        let Some(held) = &self.held else {
            return;
        };
        let rows = rows_for_height(f64::from(cx.size.height), f64::from(cx.scale), HISTORY_ROWS);
        let values = [
            self.ring.write as f32,
            HISTORY_ROWS as f32,
            rows as f32,
            start as f32,
            width as f32,
            cx.size.width as f32,
        ];
        cx.queue
            .write_buffer(&held.params, 0, &params_bytes(values, palette));
        draw(cx, &held.pipeline, &held.binding);
        self.drawn = Some(size);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        super::{colormap::Colormap, palette::DEFAULT_CUSTOM},
        *,
    };

    fn row(bins: usize) -> Pending {
        Pending::Row(vec![0; bins], RowLevel::PLAIN)
    }

    fn level(signal: f32, at_ms: f64) -> RowLevel {
        RowLevel {
            signal,
            span_db: 80.0,
            at_ms,
        }
    }

    fn rows_written(ring: &mut Ring, pending: &Pending) -> Vec<(usize, usize)> {
        let mut written = Vec::new();
        ring.apply(pending, |upload| {
            if let Upload::Rows { at, count, .. } = upload {
                written.push((at, count));
            }
        });
        written
    }

    #[test]
    fn a_row_lands_at_the_cursor_which_then_moves_on() {
        let mut ring = Ring::default();
        assert_eq!(rows_written(&mut ring, &row(FIRST_BINS)), [(0, 1)]);
        assert_eq!(rows_written(&mut ring, &row(FIRST_BINS)), [(1, 1)]);
        assert_eq!(ring.write, 2);
    }

    #[test]
    fn a_row_carries_its_signal_level_to_the_shader() {
        let mut ring = Ring::default();
        ring.apply(&Pending::Row(vec![0; FIRST_BINS], level(0.25, 1.0)), |_| {});
        assert_eq!(ring.rows[0], [0.0, 0.25, 80.0, 0.0]);
        let mut bytes = Vec::new();
        ring.row_bytes(&mut bytes);
        assert_eq!(bytes.len(), HISTORY_ROWS * ROW_FIELDS * 4);
        assert_eq!(&bytes[4..8], &0.25f32.to_le_bytes());
    }

    #[test]
    fn a_new_bin_count_clears_the_ring_first() {
        let mut ring = Ring::default();
        ring.apply(&row(FIRST_BINS), |_| {});
        let mut reallocated = None;
        ring.apply(&row(8), |upload| {
            if let Upload::Reallocate(bins) = upload {
                reallocated = Some(bins);
            }
        });
        assert_eq!(reallocated, Some(8));
        assert_eq!(ring.write, 1);
    }

    #[test]
    fn a_seed_keeps_the_newest_rows_and_leaves_the_cursor_above_them() {
        let mut ring = Ring::default();
        let written = rows_written(
            &mut ring,
            &Pending::Seed {
                rows: vec![0u8; 3 * 4],
                count: 3,
                bins: 4,
                levels: vec![level(0.1, 1.0), level(0.2, 2.0), level(0.3, 3.0)],
            },
        );
        assert_eq!(written, [(0, 3)]);
        assert_eq!((ring.bins, ring.write), (4, 3));
        assert_eq!(ring.rows[2][1], 0.3);
    }

    #[test]
    fn a_shift_moves_every_row_and_a_new_row_starts_unshifted() {
        let mut ring = Ring::default();
        ring.apply(&Pending::Shift(0.25), |_| {});
        assert!(ring.rows.iter().all(|row| row[0] == 0.25));
        ring.apply(&row(FIRST_BINS), |_| {});
        assert_eq!(ring.rows[0][0], 0.0);
        assert_eq!(ring.rows[1][0], 0.25);
    }

    #[test]
    fn the_feed_drops_rows_a_seed_replaces_and_ignores_no_ops() {
        let mut feed = WaterfallFeed::default();
        feed.push_row(&[1, 2], level(0.0, 7.0));
        assert_eq!(feed.clock.at(0), 7.0);
        feed.shift_rows(0.0);
        feed.shift_rows(0.1);
        feed.seed(vec![0; 4], 2, 2, vec![level(0.0, 3.0), level(0.0, 4.0)]);
        assert_eq!(feed.pending.len(), 2);
        assert!(matches!(feed.pending[0], Pending::Shift(_)));
        assert_eq!(feed.clock.at(0), 4.0);
        feed.changed = false;
        feed.set_window(0.0, 1.0);
        feed.set_palette(Palette::Map(Colormap::Classic));
        assert!(!feed.changed);
        feed.set_palette(Palette::Map(Colormap::Viridis));
        assert!(feed.changed);
    }

    #[test]
    fn the_shader_parameters_pack_six_floats_the_map_and_the_custom_stops() {
        let bytes = params_bytes(
            [1.0, 1024.0, 300.0, 0.25, 0.5, 640.0],
            Palette::Custom(DEFAULT_CUSTOM),
        );
        assert_eq!(bytes.len(), 112);
        assert_eq!(&bytes[..4], &1.0f32.to_le_bytes());
        assert_eq!(&bytes[20..24], &640.0f32.to_le_bytes());
        assert_eq!(&bytes[24..28], &CUSTOM_INDEX.to_le_bytes());
        let last = PARAMS_HEAD + 4 * 16;
        assert_eq!(&bytes[last..last + 4], &1.0f32.to_le_bytes());
        let plain = params_bytes([0.0; 6], Palette::Map(Colormap::Viridis));
        assert_eq!(&plain[24..28], &4u32.to_le_bytes());
    }

    #[test]
    fn a_ready_future_settles_at_once() {
        assert_eq!(settled(std::future::ready(3)), Some(3));
        assert_eq!(settled(std::future::pending::<u8>()), None);
    }

    #[test]
    fn the_waterfall_shader_validates() {
        use wgpu::naga::{front::wgsl, valid};
        let module = wgsl::parse_str(&waterfall_source()).expect("the waterfall shader parses");
        let checked =
            valid::Validator::new(valid::ValidationFlags::all(), valid::Capabilities::all())
                .validate(&module);
        assert!(checked.is_ok(), "{checked:?}");
    }
}

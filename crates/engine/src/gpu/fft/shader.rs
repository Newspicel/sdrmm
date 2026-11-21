use std::f64::consts::TAU;

use super::{Layout, Twiddles};

/// The transforms one generated module computes.
#[derive(Clone, Copy, Debug)]
pub(super) struct Shape {
    pub(super) size: usize,
    pub(super) batches: usize,
    pub(super) offset: usize,
    pub(super) inverse: bool,
    pub(super) loaded: bool,
    pub(super) in_place: bool,
}

/// One compute entry point and the buffers it binds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Entry {
    pub(super) name: String,
    pub(super) data: bool,
    pub(super) scratch: bool,
    pub(super) table: bool,
    pub(super) loads: bool,
}

/// WGSL for a Stockham autosort FFT: a shared-memory kernel for the first stages, then one
/// global pass per remaining radix. Sizes, strides and radices are baked in as literals so
/// every index is a shift or mask and every butterfly is unrolled.
pub(super) fn generate(
    layout: &Layout,
    shape: Shape,
    twiddles: Twiddles,
    loader: Option<&str>,
) -> (String, Vec<Entry>) {
    let mut writer = Writer {
        text: prelude(shape, twiddles, loader.unwrap_or_default()),
        shape,
        twiddles,
        used_table: false,
    };
    let mut entries = vec![writer.local(layout)];
    let mut stride = layout.tile;
    let count = layout.passes.len();
    for (index, &radix) in layout.passes.iter().enumerate() {
        let reads_scratch = (count - index) % 2 == 1;
        entries.push(writer.global(index, radix, stride, reads_scratch));
        stride *= radix;
    }
    (writer.text, entries)
}

fn prelude(shape: Shape, twiddles: Twiddles, loader: &str) -> String {
    let turn = match twiddles {
        Twiddles::Computed => "fn turn(index: u32, scale: f32) -> vec2<f32> {
    let angle = f32(index) * scale;
    return vec2<f32>(cos(angle), sin(angle));
}
"
        .to_owned(),
        Twiddles::Table => format!(
            "fn turn(index: u32, stride: u32) -> vec2<f32> {{
    let w = twiddles[index * stride];
    return vec2<f32>(w.x, {}w.y);
}}
",
            if shape.inverse { "-" } else { "" }
        ),
    };
    format!(
        "@group(0) @binding(0) var<storage, read_write> data: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> scratch: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read> twiddles: array<vec2<f32>>;

{loader}
fn mul(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {{
    return vec2<f32>(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
}}

{turn}"
    )
}

struct Writer {
    text: String,
    shape: Shape,
    twiddles: Twiddles,
    used_table: bool,
}

impl Writer {
    fn line(&mut self, line: &str) {
        self.text.push_str("    ");
        self.text.push_str(line);
        self.text.push('\n');
    }

    fn local(&mut self, layout: &Layout) -> Entry {
        self.used_table = false;
        let span = layout.tile * layout.columns;
        self.text += &format!(
            "var<workgroup> tile: array<vec2<f32>, {}>;

fn at(index: u32) -> u32 {{
    return index + (index >> {PAD_SHIFT}u);
}}

@compute @workgroup_size({})
fn tiles(
    @builtin(local_invocation_index) t: u32,
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(num_workgroups) wn: vec3<u32>,
) {{
",
            span + (span >> PAD_SHIFT),
            layout.threads
        );
        self.line("let g = wid.x + wid.y * wn.x;");
        self.line(&format!("if (g >= {}u) {{ return; }}", layout.groups));
        if layout.strided() {
            self.strided_load(layout);
        } else {
            self.whole_copy(layout, true);
        }
        self.line("workgroupBarrier();");
        let mut stride = 1;
        for &radix in &layout.radices {
            self.shared_pass(layout, radix, stride);
            stride *= radix;
        }
        let into_scratch = layout.passes.len() % 2 == 1;
        if layout.strided() {
            self.strided_store(layout, into_scratch);
        } else {
            self.whole_copy(layout, false);
        }
        self.text += "}\n\n";
        let loaded = self.shape.loaded;
        Entry {
            name: "tiles".to_owned(),
            data: self.shape.in_place || !into_scratch,
            scratch: into_scratch,
            table: self.used_table,
            loads: loaded,
        }
    }

    fn whole_copy(&mut self, layout: &Layout, load: bool) {
        let (size, batches) = (self.shape.size, self.shape.batches);
        if load {
            self.line(&format!("let first = g * {}u;", layout.columns));
            self.line(&format!(
                "let base = {}u + first * {size}u;",
                self.shape.offset
            ));
        }
        for m in 0..layout.elements() {
            let e = format!("t + {}u", m * layout.threads);
            let copy = if !load {
                format!("data[base + {e}] = tile[at({e})];")
            } else if self.shape.loaded {
                format!("tile[at({e})] = sample(first + ({e}) / {size}u, ({e}) % {size}u);")
            } else {
                format!("tile[at({e})] = data[base + {e}];")
            };

            self.line(&format!(
                "if (first + ({e}) / {size}u < {batches}u) {{ {copy} }}"
            ));
        }
    }

    fn strided_load(&mut self, layout: &Layout) {
        let (size, columns, tile) = (self.shape.size, layout.columns, layout.tile);
        let stride = size / tile;
        let per = stride / columns;
        self.line(&format!("let transform = g / {per}u;"));
        self.line(&format!("let c0 = (g % {per}u) * {columns}u;"));
        self.line(&format!(
            "let base = {}u + transform * {size}u + c0;",
            self.shape.offset
        ));
        let read = if self.shape.loaded {
            format!("sample(transform, c0 + c + i * {stride}u)")
        } else {
            format!("data[base + c + i * {stride}u]")
        };
        for m in 0..layout.elements() {
            self.line(&format!(
                "{{ let e = t + {}u; let c = e % {columns}u; let i = e / {columns}u; tile[at(c * {tile}u + i)] = {read}; }}",
                m * layout.threads
            ));
        }
    }

    fn strided_store(&mut self, layout: &Layout, into_scratch: bool) {
        let (target, offset) = if into_scratch {
            ("scratch", 0)
        } else {
            ("data", self.shape.offset)
        };
        self.line(&format!(
            "let out = {offset}u + transform * {}u + c0 * {}u;",
            self.shape.size, layout.tile
        ));
        for m in 0..layout.elements() {
            let e = format!("t + {}u", m * layout.threads);
            self.line(&format!("{target}[out + {e}] = tile[at({e})];"));
        }
    }

    fn shared_pass(&mut self, layout: &Layout, radix: usize, stride: usize) {
        let (tile, threads) = (layout.tile, layout.threads);
        let quarter = tile / radix;
        self.line("{");
        let groups = layout.elements() / radix;
        for b in 0..groups {
            self.line(&format!("let q{b} = t + {}u;", b * threads));
            self.line(&format!("let c{b} = q{b} / {quarter}u * {tile}u;"));
            self.line(&format!("let j{b} = q{b} % {quarter}u;"));
            self.line(&format!("let k{b} = j{b} % {stride}u;"));
            let names = names(&format!("v{b}_"), radix);
            for (m, name) in names.iter().enumerate() {
                self.line(&format!(
                    "var {name} = tile[at(c{b} + j{b} + {}u)];",
                    m * quarter
                ));
            }
            self.twiddle(&names, &format!("k{b}"), stride * radix);
            self.butterfly(&names);
        }
        self.line("workgroupBarrier();");
        for b in 0..groups {
            self.line(&format!(
                "let o{b} = c{b} + (j{b} - k{b}) * {radix}u + k{b};"
            ));
            let names = names(&format!("v{b}_"), radix);
            for m in 0..radix {
                self.line(&format!(
                    "tile[at(o{b} + {}u)] = {};",
                    m * stride,
                    names[reversed(m, radix)]
                ));
            }
        }
        self.line("workgroupBarrier();");
        self.line("}");
    }

    fn global(&mut self, index: usize, radix: usize, stride: usize, reads_scratch: bool) -> Entry {
        self.used_table = false;
        let Shape {
            size,
            batches,
            offset,
            ..
        } = self.shape;
        let quarter = size / radix;
        let (source, target) = if reads_scratch {
            (("scratch", 0), ("data", offset))
        } else {
            (("data", offset), ("scratch", 0))
        };
        self.text += &format!(
            "@compute @workgroup_size({GLOBAL_THREADS})
fn pass{index}(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) wn: vec3<u32>) {{
"
        );
        self.line(&format!("let q = id.x + id.y * wn.x * {GLOBAL_THREADS}u;"));
        self.line(&format!("if (q >= {}u) {{ return; }}", batches * quarter));
        self.line(&format!("let base = q / {quarter}u * {size}u;"));
        self.line(&format!("let j = q % {quarter}u;"));
        self.line(&format!("let k = j % {stride}u;"));
        self.line(&format!("let read = {}u + base + j;", source.1));
        let names = names("v", radix);
        for (m, name) in names.iter().enumerate() {
            self.line(&format!(
                "var {name} = {}[read + {}u];",
                source.0,
                m * quarter
            ));
        }
        self.twiddle(&names, "k", stride * radix);
        self.butterfly(&names);
        self.line(&format!(
            "let write = {}u + base + (j - k) * {radix}u + k;",
            target.1
        ));
        for m in 0..radix {
            self.line(&format!(
                "{}[write + {}u] = {};",
                target.0,
                m * stride,
                names[reversed(m, radix)]
            ));
        }
        self.text += "}\n\n";
        Entry {
            name: format!("pass{index}"),
            data: true,
            scratch: true,
            table: self.used_table,
            loads: false,
        }
    }

    /// Multiplies input `m` by W^(m k) of a transform of `span` points.
    fn twiddle(&mut self, names: &[String], k: &str, span: usize) {
        if span <= names.len() {
            return;
        }
        let argument = match self.twiddles {
            Twiddles::Computed => float(sign(self.shape.inverse) * TAU / span as f64),
            Twiddles::Table => {
                self.used_table = true;
                format!("{}u", self.shape.size / span)
            }
        };
        for (m, name) in names.iter().enumerate().skip(1) {
            self.line(&format!(
                "{name} = mul({name}, turn({k} * {m}u, {argument}));"
            ));
        }
    }

    /// Radix-2 decimation in frequency over registers; output `k` lands in `names[reversed(k)]`.
    fn butterfly(&mut self, names: &[String]) {
        let mut half = names.len() / 2;
        while half >= 1 {
            for group in (0..names.len()).step_by(2 * half) {
                for i in 0..half {
                    let (a, b) = (&names[group + i], &names[group + i + half]);
                    let rotated = rotate("d", i, 2 * half, self.shape.inverse);
                    let line =
                        format!("{{ let d = {a} - {b}; {a} = {a} + {b}; {b} = {rotated}; }}");
                    self.line(&line);
                }
            }
            half /= 2;
        }
    }
}

const GLOBAL_THREADS: usize = 256;
/// One padding slot every `2^PAD_SHIFT` values spreads strided shared-memory accesses over banks.
const PAD_SHIFT: usize = 4;

pub(super) const fn global_threads() -> usize {
    GLOBAL_THREADS
}

fn names(prefix: &str, count: usize) -> Vec<String> {
    (0..count).map(|m| format!("{prefix}{m}")).collect()
}

fn reversed(index: usize, radix: usize) -> usize {
    index.reverse_bits() >> (usize::BITS - radix.trailing_zeros())
}

fn sign(inverse: bool) -> f64 {
    if inverse { 1.0 } else { -1.0 }
}

fn float(value: f64) -> String {
    format!("{:?}", value as f32)
}

fn rotate(x: &str, index: usize, span: usize, inverse: bool) -> String {
    if index == 0 {
        return x.to_owned();
    }
    if index * 4 == span {
        return if inverse {
            format!("vec2<f32>(-{x}.y, {x}.x)")
        } else {
            format!("vec2<f32>({x}.y, -{x}.x)")
        };
    }
    let angle = sign(inverse) * TAU * index as f64 / span as f64;
    format!(
        "mul({x}, vec2<f32>({}, {}))",
        float(angle.cos()),
        float(angle.sin())
    )
}

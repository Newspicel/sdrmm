use super::{Kernel, ParityCheck};

const TARGET: f32 = 1_024.0;
const CEILING: f32 = i16::MAX as f32;
const LANES: usize = 16;

/// One `size`-square circulant of a quasi-cyclic parity-check matrix: row `lane` of its layer
/// checks variable `base + (lane + shift) % size`. An `open` circulant leaves out the rows
/// that wrap around, as the first column of an accumulator staircase does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Circulant {
    pub base: usize,
    pub shift: usize,
    pub open: bool,
}

impl Circulant {
    /// The variable row `lane` checks, if any.
    #[must_use]
    pub const fn variable(self, lane: usize, size: usize) -> Option<usize> {
        if self.open && lane < size - self.shift {
            None
        } else {
            Some(self.base + (lane + self.shift) % size)
        }
    }
}

/// A quasi-cyclic code as layers of circulants. Layer `l` holds checks `l * size..(l + 1) *
/// size` in [`QcLayout::check`] order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QcLayout {
    size: usize,
    variables: usize,
    circulants: Vec<Circulant>,
    bounds: Vec<usize>,
    degree: usize,
}

impl QcLayout {
    /// `None` when a circulant reaches past `variables` or shifts by a full turn.
    #[must_use]
    pub fn new(size: usize, variables: usize, layers: Vec<Vec<Circulant>>) -> Option<Self> {
        let fits =
            |circulant: &Circulant| circulant.shift < size && circulant.base + size <= variables;
        if size == 0 || layers.iter().flatten().any(|circulant| !fits(circulant)) {
            return None;
        }
        let degree = layers.iter().map(Vec::len).max().unwrap_or(0);
        let mut bounds = Vec::with_capacity(layers.len() + 1);
        bounds.push(0);
        let mut circulants = Vec::new();
        for layer in layers {
            circulants.extend(layer);
            bounds.push(circulants.len());
        }
        Some(Self {
            size,
            variables,
            circulants,
            bounds,
            degree,
        })
    }

    #[must_use]
    pub const fn size(&self) -> usize {
        self.size
    }

    #[must_use]
    pub const fn layers(&self) -> usize {
        self.bounds.len() - 1
    }

    #[must_use]
    pub const fn variables(&self) -> usize {
        self.variables
    }

    #[must_use]
    pub fn layer(&self, layer: usize) -> &[Circulant] {
        &self.circulants[self.bounds[layer]..self.bounds[layer + 1]]
    }

    #[must_use]
    pub const fn check(&self, layer: usize, lane: usize) -> usize {
        layer * self.size + lane
    }

    /// Every `(check, variable)` pair the circulants describe.
    pub fn pairs(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        (0..self.layers()).flat_map(move |layer| {
            self.layer(layer).iter().flat_map(move |&circulant| {
                (0..self.size).filter_map(move |lane| {
                    Some((
                        self.check(layer, lane),
                        circulant.variable(lane, self.size)?,
                    ))
                })
            })
        })
    }

    /// The same code as a plain sparse matrix.
    #[must_use]
    pub fn parity_check(&self) -> ParityCheck {
        let pairs: Vec<_> = self.pairs().collect();
        ParityCheck::from_edges(self.layers() * self.size, self.variables, &pairs)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Sweep {
    unsatisfied: u32,
    flipped: bool,
}

/// Layered normalized min-sum (scale 7/8) on 16-bit fixed point, all rows of a circulant at
/// once. Input log-likelihood ratios are scaled so their mean magnitude is 1024, which keeps
/// small values resolvable and large ones clear of saturation.
pub struct QuasiCyclic {
    layout: QcLayout,
    kernel: Kernel,
    stride: usize,
    totals: Vec<i16>,
    messages: Vec<i16>,
    gathered: Vec<i16>,
    extrinsic: Vec<i16>,
}

impl QuasiCyclic {
    #[must_use]
    pub fn new(layout: QcLayout) -> Self {
        let stride = layout.size.next_multiple_of(LANES);
        let scratch = layout.degree * stride;
        Self {
            messages: vec![0; layout.circulants.len() * stride],
            gathered: vec![0; scratch],
            extrinsic: vec![0; scratch],
            totals: vec![0; layout.variables],
            kernel: Kernel::detect(),
            stride,
            layout,
        }
    }

    #[must_use]
    pub const fn layout(&self) -> &QcLayout {
        &self.layout
    }

    pub const fn set_kernel(&mut self, kernel: Kernel) {
        self.kernel = kernel;
    }

    /// The quantized posterior per variable.
    #[must_use]
    pub fn totals(&self) -> &[i16] {
        &self.totals
    }

    /// Starts a new word; `place` maps an index into `llrs` to its variable.
    pub fn load(&mut self, llrs: &[f32], place: impl Fn(usize) -> usize) {
        let scale = scale(llrs);
        for (bit, &llr) in llrs.iter().enumerate() {
            self.totals[place(bit)] = quantize(llr, scale);
        }
        self.messages.fill(0);
    }

    /// Starts a new word whose first `information` bits sit linearly and whose parity bits are
    /// stored column-wise across `layers` rows of `layout.size()` lanes.
    pub fn load_column_parity(&mut self, llrs: &[f32], information: usize, layers: usize) {
        let scale = scale(llrs);
        let group = self.layout.size;
        quantize_slice(&llrs[..information], scale, &mut self.totals[..information]);
        let parity_llrs = &llrs[information..];
        let parity_totals = &mut self.totals[information..];
        let mut cursor = 0;
        for lane in 0..group {
            for layer in 0..layers {
                parity_totals[layer * group + lane] = quantize(parity_llrs[cursor], scale);
                cursor += 1;
            }
        }
        self.messages.fill(0);
    }

    /// Sweeps every layer until a sweep finds every check holding and flips no total, at most
    /// `limit + 1` sweeps. Returns the sweeps that changed something.
    pub fn run(&mut self, limit: usize) -> Option<usize> {
        (0..=limit).find(|_| {
            let sweep = self.sweep();
            sweep.unsatisfied == 0 && !sweep.flipped
        })
    }

    /// The hard decisions of the first `hard.len()` variables.
    pub fn harden(&self, hard: &mut [bool]) {
        for (bit, &total) in hard.iter_mut().zip(&self.totals) {
            *bit = total < 0;
        }
    }

    fn sweep(&mut self) -> Sweep {
        let mut sweep = Sweep::default();
        for layer in 0..self.layout.layers() {
            let outcome = self.update(layer);
            sweep.unsatisfied += outcome.unsatisfied;
            sweep.flipped |= outcome.flipped;
        }
        sweep
    }

    fn update(&mut self, layer: usize) -> Sweep {
        let stride = self.stride;
        let size = self.layout.size;
        let circulants = self.layout.layer(layer);
        let rows = circulants.len() * stride;
        let first = self.layout.bounds[layer] * stride;
        let messages = &mut self.messages[first..first + rows];
        for (circulant, row) in circulants
            .iter()
            .zip(self.gathered.chunks_exact_mut(stride))
        {
            gather(&self.totals, *circulant, size, row);
        }
        let unsatisfied = self.kernel.update(
            stride,
            &mut self.gathered[..rows],
            &mut self.extrinsic[..rows],
            messages,
        );
        let mut flips = 0i16;
        for ((circulant, delta), message) in circulants
            .iter()
            .zip(self.gathered.chunks_exact(stride))
            .zip(messages.chunks_exact_mut(stride))
        {
            flips |= scatter(&mut self.totals, *circulant, size, delta, message);
        }
        Sweep {
            unsatisfied,
            flipped: flips < 0,
        }
    }
}

fn scale(llrs: &[f32]) -> f32 {
    #[cfg(target_arch = "aarch64")]
    let sum = {
        use std::arch::aarch64::{
            vabsq_f32, vaddq_f32, vaddvq_f32, vdupq_n_f32, vld1q_f32, vminq_f32,
        };
        let (blocks, rest) = llrs.as_chunks::<16>();
        unsafe {
            let cap = vdupq_n_f32(CEILING);
            let mut s0 = vdupq_n_f32(0.0);
            let mut s1 = vdupq_n_f32(0.0);
            let mut s2 = vdupq_n_f32(0.0);
            let mut s3 = vdupq_n_f32(0.0);
            for block in blocks {
                let p = block.as_ptr();
                s0 = vaddq_f32(s0, vminq_f32(vabsq_f32(vld1q_f32(p)), cap));
                s1 = vaddq_f32(s1, vminq_f32(vabsq_f32(vld1q_f32(p.add(4))), cap));
                s2 = vaddq_f32(s2, vminq_f32(vabsq_f32(vld1q_f32(p.add(8))), cap));
                s3 = vaddq_f32(s3, vminq_f32(vabsq_f32(vld1q_f32(p.add(12))), cap));
            }
            let tail: f32 = rest.iter().map(|llr| llr.abs().min(CEILING)).sum();
            vaddvq_f32(vaddq_f32(vaddq_f32(s0, s1), vaddq_f32(s2, s3))) + tail
        }
    };
    #[cfg(not(target_arch = "aarch64"))]
    let sum: f32 = {
        let mut acc = [0.0f32; 4];
        let (quads, tail) = llrs.as_chunks::<4>();
        for q in quads {
            for k in 0..4 {
                acc[k] += q[k].abs().min(CEILING);
            }
        }
        acc.iter().sum::<f32>() + tail.iter().map(|llr| llr.abs().min(CEILING)).sum::<f32>()
    };
    let mean = sum / llrs.len().max(1) as f32;
    if mean.is_normal() { TARGET / mean } else { 1.0 }
}

#[inline(always)]
fn quantize(llr: f32, scale: f32) -> i16 {
    (llr * scale).round().clamp(-CEILING, CEILING) as i16
}

fn quantize_slice(llrs: &[f32], scale: f32, out: &mut [i16]) {
    let len = llrs.len().min(out.len());
    let mut done = 0;
    #[cfg(target_arch = "aarch64")]
    {
        use std::arch::aarch64::{
            vcombine_s16, vcvtq_s32_f32, vdupq_n_f32, vld1q_f32, vmaxq_f32, vminq_f32, vmovn_s32,
            vmulq_f32, vrndaq_f32, vst1q_s16,
        };
        let n = len & !7;
        unsafe {
            let sp = llrs.as_ptr();
            let dp = out.as_mut_ptr();
            let vs = vdupq_n_f32(scale);
            let hi = vdupq_n_f32(CEILING);
            let lo = vdupq_n_f32(-CEILING);
            while done < n {
                let a = vminq_f32(
                    vmaxq_f32(vrndaq_f32(vmulq_f32(vld1q_f32(sp.add(done)), vs)), lo),
                    hi,
                );
                let b = vminq_f32(
                    vmaxq_f32(vrndaq_f32(vmulq_f32(vld1q_f32(sp.add(done + 4)), vs)), lo),
                    hi,
                );
                let packed = vcombine_s16(vmovn_s32(vcvtq_s32_f32(a)), vmovn_s32(vcvtq_s32_f32(b)));
                vst1q_s16(dp.add(done), packed);
                done += 8;
            }
        }
    }
    for (dst, &llr) in out[done..len].iter_mut().zip(&llrs[done..len]) {
        *dst = quantize(llr, scale);
    }
}

fn gather(totals: &[i16], circulant: Circulant, size: usize, row: &mut [i16]) {
    let split = size - circulant.shift;
    let source = &totals[circulant.base..circulant.base + size];
    if circulant.open {
        row[..split].fill(i16::MAX);
    } else {
        row[..split].copy_from_slice(&source[circulant.shift..]);
    }
    row[split..size].copy_from_slice(&source[..circulant.shift]);
    row[size..].fill(i16::MAX);
}

fn scatter(
    totals: &mut [i16],
    circulant: Circulant,
    size: usize,
    delta: &[i16],
    message: &mut [i16],
) -> i16 {
    let split = size - circulant.shift;
    let target = &mut totals[circulant.base..circulant.base + size];
    let (head, tail) = target.split_at_mut(circulant.shift);
    let flips = if circulant.open {
        message[..split].fill(0);
        0
    } else {
        accumulate(tail, &delta[..split])
    };
    flips | accumulate(head, &delta[split..size])
}

fn accumulate(target: &mut [i16], delta: &[i16]) -> i16 {
    let len = target.len().min(delta.len());
    let mut flips = 0i16;
    let mut done = 0;
    #[cfg(target_arch = "aarch64")]
    {
        use std::arch::aarch64::{
            vdupq_n_s16, veorq_s16, vld1q_s16, vorrq_s16, vqaddq_s16, vst1q_s16,
        };
        let n = len & !15;
        unsafe {
            let tp = target.as_mut_ptr();
            let dp = delta.as_ptr();
            let mut f0 = vdupq_n_s16(0);
            let mut f1 = vdupq_n_s16(0);
            while done < n {
                let t0 = vld1q_s16(tp.add(done));
                let t1 = vld1q_s16(tp.add(done + 8));
                let u0 = vqaddq_s16(t0, vld1q_s16(dp.add(done)));
                let u1 = vqaddq_s16(t1, vld1q_s16(dp.add(done + 8)));
                f0 = vorrq_s16(f0, veorq_s16(t0, u0));
                f1 = vorrq_s16(f1, veorq_s16(t1, u1));
                vst1q_s16(tp.add(done), u0);
                vst1q_s16(tp.add(done + 8), u1);
                done += 16;
            }
            while done + 8 <= len {
                let t = vld1q_s16(tp.add(done));
                let u = vqaddq_s16(t, vld1q_s16(dp.add(done)));
                f0 = vorrq_s16(f0, veorq_s16(t, u));
                vst1q_s16(tp.add(done), u);
                done += 8;
            }
            let combined = vorrq_s16(f0, f1);
            let mut lanes = [0i16; 8];
            vst1q_s16(lanes.as_mut_ptr(), combined);
            for lane in lanes {
                flips |= lane;
            }
        }
    }
    for (total, &change) in target[done..len].iter_mut().zip(&delta[done..len]) {
        let updated = total.saturating_add(change);
        flips |= *total ^ updated;
        *total = updated;
    }
    flips
}

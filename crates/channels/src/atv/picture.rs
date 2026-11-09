use std::f32::consts::FRAC_PI_2;

use num_complex::Complex;

use super::{
    AtvColor,
    standard::Geometry,
    sync::{Levels, Samples},
};
use crate::VideoPicture;

const PHASES: usize = 32;
/// Luma blur in seconds of Gaussian sigma: the sharpest is about 4 MHz, the softest keeps the
/// 10 to 90 % rise near 400 ns.
const SHARPEST_S: f64 = 34e-9;
const SOFTEST_S: f64 = 135e-9;
const AUTO_STEPS: usize = 8;
/// Luma noise, as a share of black to white, that auto bandwidth settles for.
const NOISE_TARGET: f32 = 0.005;
const NOISE_TRACK: f32 = 0.05;
const NOISE_EVERY: u32 = 4;
const SIGMA_PER_HZ: f64 = 0.1325;
/// Gaussian noise's RMS over its mean absolute deviation; clicks sway it far less.
const ABS_TO_RMS: f32 = 1.2533;
/// Luma noise, as a share of black to white, above which narrow bumps are taken for clicks.
const REPAIR_NOISE: f32 = 0.008;
const REPAIR_SIGMAS: f32 = 6.0;
const REPAIR_REACH: f64 = 2.5;
const SOUND_TRAP: f64 = 0.42;
const MARGIN: usize = 4;
const BURST_GUARD_US: f64 = 0.1;
const COLOUR_ON: f32 = 0.05;
const COLOUR_OFF: f32 = 0.03;
const PHASE_GAIN: f32 = 0.2;
const DRIFT_GAIN: f32 = 0.01;
const AMPLITUDE_TRACK: f32 = 0.05;
const IDENT_TRACK: f32 = 0.3;

/// Gaussian low-passes for luma, each cut into fractional-delay phases.
#[derive(Clone, Debug)]
struct Blur {
    sigma: f64,
    half: usize,
    taps: usize,
    table: Vec<f32>,
}

impl Blur {
    fn new(sigma: f64) -> Self {
        let half = (3.0 * sigma).ceil().max(1.0) as usize + 1;
        let taps = 2 * half;
        let mut table = vec![0.0f32; PHASES * taps];
        for (phase, row) in table.chunks_exact_mut(taps).enumerate() {
            let frac = phase as f64 / PHASES as f64;
            let weights: Vec<f64> = (0..taps)
                .map(|k| {
                    let d = k as f64 - (half as f64 - 1.0) - frac;
                    (-0.5 * (d / sigma).powi(2)).exp()
                })
                .collect();
            let sum: f64 = weights.iter().sum();
            for (slot, w) in row.iter_mut().zip(weights) {
                *slot = (w / sum) as f32;
            }
        }
        Self {
            sigma,
            half,
            taps,
            table,
        }
    }

    fn row(&self, frac: f64) -> &[f32] {
        let phase = ((frac * PHASES as f64) as usize).min(PHASES - 1);
        &self.table[phase * self.taps..(phase + 1) * self.taps]
    }

    /// `out[k]` is the blurred signal at `x[k + half - 1 + frac]`.
    fn apply(&self, x: &[f32], frac: f64, out: &mut [f32]) {
        out.fill(0.0);
        for (j, &tap) in self.row(frac).iter().enumerate() {
            for (o, &v) in out.iter_mut().zip(&x[j..]) {
                *o = tap.mul_add(v, *o);
            }
        }
    }
}

/// Picks the luma blur: fixed, or the sharpest whose noise stays under target.
#[derive(Clone, Debug)]
struct Sharpness {
    blurs: Vec<Blur>,
    noise: Vec<f32>,
    chosen: usize,
    auto: bool,
    count: u32,
}

impl Sharpness {
    fn new(rate: f64, bandwidth_hz: Option<f64>, sound_hz: Option<f64>) -> Self {
        let floor = sound_hz.map_or(0.0, |hz| SOUND_TRAP / hz);
        let sigmas: Vec<f64> = match bandwidth_hz {
            Some(hz) => vec![(SIGMA_PER_HZ / hz).max(floor)],
            None => (0..AUTO_STEPS)
                .map(|k| {
                    let t = k as f64 / (AUTO_STEPS - 1) as f64;
                    (SHARPEST_S * (SOFTEST_S / SHARPEST_S).powf(t)).max(floor)
                })
                .collect(),
        };
        let blurs: Vec<Blur> = sigmas
            .iter()
            .map(|s| Blur::new((s * rate).max(0.35)))
            .collect();
        Self {
            noise: vec![0.0; blurs.len()],
            blurs,
            chosen: 0,
            auto: bandwidth_hz.is_none(),
            count: 0,
        }
    }

    fn blur(&self) -> &Blur {
        &self.blurs[self.chosen]
    }

    /// Luma noise under the chosen blur, as a share of black to white.
    fn noise(&self) -> f32 {
        self.noise[self.chosen]
    }

    fn widest(&self) -> usize {
        self.blurs.iter().map(|b| b.taps).max().unwrap_or(2)
    }

    /// Measures luma noise on a flat stretch of the line under every candidate blur.
    fn sample_noise(&mut self, flat: &[f32], scratch: &mut Vec<f32>) {
        self.count += 1;
        if !self.count.is_multiple_of(NOISE_EVERY) {
            return;
        }
        for (blur, noise) in self.blurs.iter().zip(&mut self.noise) {
            let Some(len) = flat.len().checked_sub(blur.taps) else {
                continue;
            };
            if len < 4 {
                continue;
            }
            scratch.resize(len, 0.0);
            blur.apply(flat, 0.0, scratch);
            let mean = scratch.iter().sum::<f32>() / len as f32;
            let spread = scratch.iter().map(|v| (v - mean).abs()).sum::<f32>() / len as f32;
            *noise += NOISE_TRACK * (ABS_TO_RMS * spread - *noise);
        }
    }

    fn settle(&mut self) {
        if !self.auto {
            return;
        }
        let target = NOISE_TARGET;
        self.chosen = self
            .noise
            .iter()
            .position(|&n| n <= target)
            .unwrap_or(self.blurs.len() - 1);
    }
}

/// Burst-locked subcarrier phase, the PAL switch and the colour killer.
#[derive(Clone, Debug)]
struct Burst {
    pal: bool,
    theta: f32,
    drift: f32,
    previous: Complex<f32>,
    amplitude: f32,
    on: bool,
    ident: f32,
    sign: f32,
}

impl Burst {
    fn new(pal: bool) -> Self {
        Self {
            pal,
            theta: 0.0,
            drift: 0.0,
            previous: Complex::new(0.0, 0.0),
            amplitude: 0.0,
            on: false,
            ident: 0.0,
            sign: 1.0,
        }
    }

    /// The rotation that puts the reference burst on +V - jU's imaginary axis, so
    /// `V = re` and `U = -im`.
    fn rotation(&self) -> Complex<f32> {
        Complex::from_polar(1.0, FRAC_PI_2 - self.theta)
    }

    fn update(&mut self, burst: Complex<f32>) {
        let pair = burst + self.previous;
        self.previous = burst;
        let error = (pair * self.rotation()).arg() - FRAC_PI_2;
        if pair.norm() > COLOUR_OFF {
            if self.on {
                self.theta += PHASE_GAIN * error;
                self.drift += DRIFT_GAIN * error;
            } else {
                self.theta = pair.arg();
            }
        }
        self.theta = (self.theta + self.drift).rem_euclid(std::f32::consts::TAU);
        let coherent = (pair * self.rotation()).im;
        self.amplitude += AMPLITUDE_TRACK * (coherent - self.amplitude);
        self.on = self.amplitude > if self.on { COLOUR_OFF } else { COLOUR_ON };
        if self.pal {
            self.switch((burst * self.rotation()).re);
        }
    }

    /// A line without burst: the phase runs on and PAL keeps alternating.
    fn coast(&mut self) {
        self.theta = (self.theta + self.drift).rem_euclid(std::f32::consts::TAU);
        if self.pal {
            self.sign = -self.sign;
        }
    }

    /// PAL lines alternate the V sign; the burst swing says which this line has, and a
    /// flywheel keeps the alternation through noise and lines without burst.
    fn switch(&mut self, swing: f32) {
        let expected = -self.sign;
        self.ident += IDENT_TRACK * (swing * expected - self.ident);
        self.sign = if self.ident < 0.0 {
            self.ident = -self.ident;
            -expected
        } else {
            expected
        };
    }
}

/// Builds woven frames line by line.
pub(super) struct Picture {
    g: Geometry,
    color: AtvColor,
    sharpness: Sharpness,
    burst: Burst,
    luma: Vec<u8>,
    rgb: Vec<u8>,
    written: u32,
    fresh: bool,
    raw: Vec<f32>,
    carrier: Vec<f32>,
    even: Vec<f32>,
    odd: Vec<f32>,
    plain: Vec<f32>,
    out: Vec<f32>,
    last_u: Vec<f32>,
    last_v: Vec<f32>,
    u: Vec<f32>,
    v: Vec<f32>,
    scratch: Vec<f32>,
}

impl Picture {
    pub fn new(
        g: Geometry,
        color: AtvColor,
        bandwidth_hz: Option<f64>,
        sound_hz: Option<f64>,
    ) -> Self {
        let width = usize::from(g.width);
        let pixels = width * usize::from(g.field_lines) * 2;
        Self {
            g,
            color,
            sharpness: Sharpness::new(g.rate, bandwidth_hz, sound_hz),
            burst: Burst::new(color == AtvColor::Pal),
            luma: vec![0; pixels],
            rgb: if color == AtvColor::Monochrome {
                Vec::new()
            } else {
                vec![0; pixels * 3]
            },
            written: 0,
            fresh: true,
            raw: Vec::new(),
            carrier: Vec::new(),
            even: Vec::new(),
            odd: Vec::new(),
            plain: Vec::new(),
            out: vec![0.0; width],
            last_u: vec![0.0; width],
            last_v: vec![0.0; width],
            u: Vec::new(),
            v: Vec::new(),
            scratch: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn width(&self) -> u16 {
        self.g.width
    }

    pub fn height(&self) -> u16 {
        self.g.field_lines * 2
    }

    /// The finished field, if it drew anything.
    pub fn take(&mut self) -> Option<VideoPicture> {
        self.sharpness.settle();
        if self.written == 0 {
            return None;
        }
        self.written = 0;
        Some(VideoPicture {
            width: self.g.width,
            height: self.height(),
            luma: self.luma.clone(),
            rgb: self.rgb.clone(),
        })
    }

    fn span(levels: &Levels, g: &Geometry) -> (f32, f32) {
        let sync = levels.sync();
        let black = levels.blank + g.setup * sync;
        let white = levels.blank + g.white * sync;
        (black, (white - black).max(1e-6))
    }

    /// Keeps the colour flywheel turning over a line without line sync.
    pub fn coast(&mut self) {
        self.burst.coast();
        self.fresh = true;
    }

    /// Reads the burst and the line's noise; runs on every line with a line sync.
    pub fn observe(&mut self, samples: &Samples, zero_h: f64, levels: &Levels) {
        let (_, span) = Self::span(levels, &self.g);
        let g = self.g;
        let tip = samples.span(
            (zero_h + 0.2 * g.sync).max(0.0) as u64,
            (zero_h + 0.85 * g.sync).max(0.0) as u64,
        );
        self.scratch.clear();
        self.scratch.extend(tip.iter().map(|v| v / span));
        let flat = std::mem::take(&mut self.scratch);
        let mut work = std::mem::take(&mut self.plain);
        self.sharpness.sample_noise(&flat, &mut work);
        self.plain = work;
        self.scratch = flat;
        if self.color == AtvColor::Monochrome {
            return;
        }
        let guard = g.us(BURST_GUARD_US);
        let from = (zero_h + g.burst.0 + guard).max(0.0) as u64;
        let to = (zero_h + g.burst.1 - guard).max(0.0) as u64;
        let burst = self.demodulate(samples, from, to);
        self.burst.update(burst / span);
    }

    /// Mean subcarrier phasor over `from..to`, as `I + jQ` on the sample grid.
    fn demodulate(&mut self, samples: &Samples, from: u64, to: u64) -> Complex<f32> {
        let from_with = from.saturating_sub(3);
        let raw = samples.span(from_with, to + 3);
        if raw.len() < 8 {
            return Complex::new(0.0, 0.0);
        }
        self.raw.clear();
        self.raw.extend_from_slice(raw);
        modulate(&self.raw, from_with, &mut self.carrier);
        split(&self.carrier, &mut self.even, &mut self.odd);
        let mut sum = Complex::new(0.0, 0.0);
        for k in 3..self.carrier.len() - 3 {
            sum += phasor(from_with + k as u64, self.even[k], self.odd[k]);
        }
        sum / (self.carrier.len() - 6) as f32
    }

    /// Draws one active line into frame row `row`.
    pub fn draw(&mut self, samples: &Samples, zero_h: f64, levels: &Levels, row: usize) {
        let g = self.g;
        let width = usize::from(g.width);
        if (row + 1) * width > self.luma.len() {
            return;
        }
        let blur_half = self.sharpness.blur().half;
        let lead = blur_half + MARGIN;
        let start = zero_h + g.active.0 + 0.5;
        let first = start.floor() as u64;
        let frac = start - first as f64;
        let from = first.saturating_sub(lead as u64);
        let to = first + (width + self.sharpness.widest() + 2 * MARGIN) as u64;
        let raw = samples.span(from, to);
        if raw.len() < (to - from) as usize {
            return;
        }
        self.raw.clear();
        self.raw.extend_from_slice(raw);
        let skip = (first - from) as usize + 1 - blur_half;
        let (black, span) = Self::span(levels, &g);
        let colour = self.color != AtvColor::Monochrome;
        if colour {
            modulate(&self.raw, from, &mut self.carrier);
            split(&self.carrier, &mut self.even, &mut self.odd);
            modulate(&self.even, from, &mut self.plain);
            for (p, &r) in self.plain.iter_mut().zip(&self.raw) {
                *p = r - 2.0 * *p;
            }
        } else {
            self.plain.clear();
            self.plain.extend_from_slice(&self.raw);
        }
        let mut out = std::mem::take(&mut self.out);
        self.sharpness
            .blur()
            .apply(&self.plain[skip..], frac, &mut out);
        let noise = self.sharpness.noise();
        if noise > REPAIR_NOISE {
            let reach = (REPAIR_REACH * self.sharpness.blur().sigma)
                .round()
                .max(2.0) as usize;
            let limit = REPAIR_SIGMAS * noise * span;
            repair_clicks(&mut out, reach, limit, &mut self.scratch);
        }
        let base = row * width;
        let gain = 1.0 / span;
        for y in &mut out {
            *y = (*y - black) * gain;
        }
        for (pixel, &y) in self.luma[base..base + width].iter_mut().zip(&out) {
            *pixel = byte(y);
        }
        if colour {
            self.draw_colour(from, first + u64::from(frac >= 0.5), span, &out, row);
        }
        self.out = out;
        self.written += 1;
    }

    fn draw_colour(&mut self, from: u64, first: u64, span: f32, luma: &[f32], row: usize) {
        let width = usize::from(self.g.width);
        let offset = (first - from) as usize;
        let (even, odd) = (&self.even[offset..], &self.odd[offset..]);
        let (u, v) = (&mut self.u, &mut self.v);
        u.resize(width, 0.0);
        v.resize(width, 0.0);
        let (same, other) = if first & 1 == 0 {
            (even, odd)
        } else {
            (odd, even)
        };
        for (k, (ui, vq)) in u.iter_mut().zip(v.iter_mut()).enumerate() {
            let parity = k & 1 == 0;
            *ui = 2.0 * if parity { same[k] } else { other[k] };
            *vq = 2.0 * if parity { other[k] } else { same[k] };
        }
        let rotation = self.burst.rotation() / span;
        let (on, sign) = (self.burst.on, self.burst.sign);
        let scale = if on { 1.0 } else { 0.0 };
        for (i, q) in u.iter_mut().zip(v.iter_mut()) {
            let (re, im) = (
                *i * rotation.re - *q * rotation.im,
                *i * rotation.im + *q * rotation.re,
            );
            (*i, *q) = (-im * scale, re * sign * scale);
        }
        let average = self.burst.pal && !std::mem::replace(&mut self.fresh, false);
        for (((ui, vi), lu), lv) in u
            .iter_mut()
            .zip(v.iter_mut())
            .zip(self.last_u.iter_mut())
            .zip(self.last_v.iter_mut())
        {
            let (now_u, now_v) = (*ui, *vi);
            if average {
                (*ui, *vi) = (0.5 * (now_u + *lu), 0.5 * (now_v + *lv));
            }
            (*lu, *lv) = (now_u, now_v);
        }
        let base = row * width * 3;
        for (((pixel, &y), &ui), &vi) in self.rgb[base..base + 3 * width]
            .as_chunks_mut::<3>()
            .0
            .iter_mut()
            .zip(luma)
            .zip(u.iter())
            .zip(v.iter())
        {
            pixel[0] = byte(y + 1.140 * vi);
            pixel[1] = byte(y - 0.395 * ui - 0.581 * vi);
            pixel[2] = byte(y + 2.032 * ui);
        }
    }
}

/// `+1, -1, -1, +1` by sample index: the I and Q carriers of a 4x subcarrier grid, merged
/// since only one is non-zero at a time.
fn sign(index: u64) -> f32 {
    if (index + 1) & 2 == 0 { 1.0 } else { -1.0 }
}

fn modulate(raw: &[f32], from: u64, carrier: &mut Vec<f32>) {
    let pattern: [f32; 4] = std::array::from_fn(|k| sign(from + k as u64));
    carrier.clear();
    carrier.resize(raw.len(), 0.0);
    for (out, input) in carrier.chunks_mut(4).zip(raw.chunks(4)) {
        for ((o, &x), &s) in out.iter_mut().zip(input).zip(&pattern) {
            *o = x * s;
        }
    }
}

/// The two halves of a triangle low-pass `[1 2 3 4 3 2 1] / 16`: even taps reach samples of
/// the same parity, odd taps the other, so each half sees only one of I and Q. Its double
/// zeros at the subcarrier and twice it keep flat luma out of chroma.
fn split(carrier: &[f32], even: &mut Vec<f32>, odd: &mut Vec<f32>) {
    let n = carrier.len();
    even.clear();
    even.resize(n, 0.0);
    odd.clear();
    odd.resize(n, 0.0);
    if n < 7 {
        return;
    }
    let mid = &mut even[3..n - 3];
    for (((slot, &a), &b), &c) in mid
        .iter_mut()
        .zip(&carrier[1..n - 5])
        .zip(&carrier[3..n - 3])
        .zip(&carrier[5..n - 1])
    {
        *slot = 0.125 * (a + 2.0 * b + c);
    }
    let mid = &mut odd[3..n - 3];
    for ((((slot, &a), &b), &c), &d) in mid
        .iter_mut()
        .zip(&carrier[..n - 6])
        .zip(&carrier[2..n - 4])
        .zip(&carrier[4..n - 2])
        .zip(&carrier[6..])
    {
        *slot = 0.0625 * (a + 3.0 * (b + c) + d);
    }
}

/// `I + jQ` at `index`: the even half holds I on even samples and Q on odd ones.
fn phasor(index: u64, even: f32, odd: f32) -> Complex<f32> {
    if index & 1 == 0 {
        Complex::new(2.0 * even, 2.0 * odd)
    } else {
        Complex::new(2.0 * odd, 2.0 * even)
    }
}

/// Bridges narrow bumps that stand above both neighbours `reach` away by more than `limit`:
/// a click blurred into luma looks like that, an edge never does.
pub(super) fn repair_clicks(luma: &mut [f32], reach: usize, limit: f32, original: &mut Vec<f32>) {
    let n = luma.len();
    if n <= 2 * reach {
        return;
    }
    original.clear();
    original.extend_from_slice(luma);
    let bump = |k: usize| {
        let (left, right) = (
            original[k] - original[k - reach],
            original[k] - original[k + reach],
        );
        left * right > 0.0 && left.abs().min(right.abs()) > limit
    };
    let mut k = reach;
    while k < n - reach {
        if !bump(k) {
            k += 1;
            continue;
        }
        let first = k;
        while k + 1 < n - reach && bump(k + 1) {
            k += 1;
        }
        let (from, to) = (first - reach, k + reach);
        let (a, b) = (original[from], original[to]);
        let span = (to - from) as f32;
        for (step, value) in luma[from + 1..to].iter_mut().enumerate() {
            *value = a + (b - a) * (step + 1) as f32 / span;
        }
        k += 1;
    }
}

fn byte(value: f32) -> u8 {
    (value * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

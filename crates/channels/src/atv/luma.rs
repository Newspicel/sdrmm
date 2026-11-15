const PHASES: usize = 32;
/// Luma blur in seconds of Gaussian sigma: the sharpest is about 4 MHz, the softest keeps the
/// 10 to 90 % rise near 400 ns.
const SHARPEST_S: f64 = 34e-9;
const SOFTEST_S: f64 = 135e-9;
const AUTO_STEPS: usize = 8;
/// Luma noise, as a share of black to white, that auto bandwidth settles for.
const NOISE_TARGET: f32 = 0.005;
const NOISE_TRACK: f32 = 0.05;
/// Once no blur meets the target, the sharpest within this share of the softest's noise is
/// taken, the share shrinking as the noise grows: a little past the target, sharpness is
/// cheap; deep in the noise, every bit of blur counts.
const FLOOR_SLACK: f32 = 0.5;
const NOISE_EVERY: u32 = 4;
const SIGMA_PER_HZ: f64 = 0.1325;
/// Gaussian noise's RMS over its mean absolute deviation; clicks sway it far less.
const ABS_TO_RMS: f32 = 1.2533;
const SOUND_TRAP: f64 = 0.42;
pub(super) const NOTCH: [f32; 5] = [0.25, 0.0, 0.5, 0.0, 0.25];
const NOTCH_REACH: usize = 2;

/// Gaussian low-passes for luma, each cut into fractional-delay phases.
#[derive(Clone, Debug)]
pub(super) struct Blur {
    pub sigma: f64,
    pub half: usize,
    pub taps: usize,
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

    /// This blur after the 4x subcarrier notch `[1 0 2 0 1] / 4`, as one kernel: what is
    /// left of a composite line once the chroma the triangle low-pass finds is taken out.
    fn notched(&self) -> Self {
        let taps = self.taps + 2 * NOTCH_REACH;
        let mut table = vec![0.0f32; PHASES * taps];
        for (row, plain) in table
            .chunks_exact_mut(taps)
            .zip(self.table.chunks_exact(self.taps))
        {
            for (k, &w) in plain.iter().enumerate() {
                for (offset, &n) in NOTCH.iter().enumerate() {
                    row[k + offset] += w * n;
                }
            }
        }
        Self {
            sigma: self.sigma,
            half: self.half + NOTCH_REACH,
            taps,
            table,
        }
    }

    fn row(&self, frac: f64) -> &[f32] {
        let phase = ((frac * PHASES as f64) as usize).min(PHASES - 1);
        &self.table[phase * self.taps..(phase + 1) * self.taps]
    }

    /// `out[k]` is the blurred signal at `x[k + half - 1 + frac]`.
    pub fn apply(&self, x: &[f32], frac: f64, out: &mut [f32]) {
        let row = self.row(frac);
        let (blocks, tail) = out.as_chunks_mut::<BLUR_BLOCK>();
        for (k, block) in blocks.iter_mut().enumerate() {
            *block = blur_block(row, &x[k * BLUR_BLOCK..]);
        }
        let done = blocks.len() * BLUR_BLOCK;
        for (k, o) in tail.iter_mut().enumerate() {
            *o = row
                .iter()
                .zip(&x[done + k..])
                .fold(0.0, |sum, (&tap, &v)| sdrmm_dsp::madd(tap, v, sum));
        }
    }
}

const BLUR_BLOCK: usize = 16;

/// `BLUR_BLOCK` neighbouring outputs, held in registers while every tap passes over them.
#[inline(always)]
fn blur_block(row: &[f32], x: &[f32]) -> [f32; BLUR_BLOCK] {
    let mut sums = [0.0f32; BLUR_BLOCK];
    for (j, &tap) in row.iter().enumerate() {
        let Some(window) = x.get(j..).and_then(<[f32]>::first_chunk::<BLUR_BLOCK>) else {
            break;
        };
        for (sum, &v) in sums.iter_mut().zip(window) {
            *sum = sdrmm_dsp::madd(tap, v, *sum);
        }
    }
    sums
}

/// Picks the luma blur: fixed, or the sharpest whose noise stays under target.
#[derive(Clone, Debug)]
pub(super) struct Sharpness {
    blurs: Vec<Blur>,
    /// What each blur draws with: itself, or folded with the chroma notch for colour.
    drawn: Vec<Blur>,
    noise: Vec<f32>,
    chosen: usize,
    auto: bool,
    count: u32,
}

impl Sharpness {
    pub fn new(rate: f64, bandwidth_hz: Option<f64>, sound_hz: Option<f64>, notch: bool) -> Self {
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
        let drawn = blurs
            .iter()
            .map(|b| if notch { b.notched() } else { b.clone() })
            .collect();
        Self {
            noise: vec![0.0; blurs.len()],
            blurs,
            drawn,
            chosen: 0,
            auto: bandwidth_hz.is_none(),
            count: 0,
        }
    }

    pub fn blur(&self) -> &Blur {
        &self.drawn[self.chosen]
    }

    /// Luma noise under the chosen blur, as a share of black to white.
    pub fn noise(&self) -> f32 {
        self.noise[self.chosen]
    }

    pub fn widest(&self) -> usize {
        self.drawn.iter().map(|b| b.taps).max().unwrap_or(2)
    }

    /// Measures luma noise on a flat stretch of the line under every candidate blur.
    pub fn sample_noise(&mut self, flat: &[f32], scratch: &mut Vec<f32>) {
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

    pub fn settle(&mut self) {
        if !self.auto {
            return;
        }
        let floor = self.noise.last().copied().unwrap_or(0.0);
        let slack = 1.0 + FLOOR_SLACK * (NOISE_TARGET / floor.max(f32::MIN_POSITIVE)).min(1.0);
        let target = NOISE_TARGET.max(slack * floor);
        self.chosen = self
            .noise
            .iter()
            .position(|&n| n <= target)
            .unwrap_or(self.blurs.len() - 1);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atv::chroma::{modulate, split};

    fn line(len: usize) -> Vec<f32> {
        (0..len)
            .map(|k| ((k * 7919) % 101) as f32 / 50.0 - 1.0)
            .collect()
    }

    fn close(a: &[f32], b: &[f32]) {
        for (k, (x, y)) in a.iter().zip(b).enumerate() {
            assert!((x - y).abs() < 1e-5, "sample {k}: {x} vs {y}");
        }
    }

    #[test]
    fn the_notched_blur_is_the_chroma_split_then_the_blur() {
        let raw = line(200);
        let from = 13;
        let (mut carrier, mut even, mut odd, mut chroma) = (vec![], vec![], vec![], vec![]);
        modulate(&raw, from, &mut carrier);
        split(&carrier, &mut even, &mut odd);
        modulate(&even, from, &mut chroma);
        let luma: Vec<f32> = raw.iter().zip(&chroma).map(|(r, c)| r - 2.0 * c).collect();
        let blur = Blur::new(1.7);
        let notched = blur.notched();
        for frac in [0.0, 0.3, 0.9] {
            let (mut want, mut got) = (vec![0.0; 150], vec![0.0; 150]);
            blur.apply(&luma[10..], frac, &mut want);
            notched.apply(&raw[8..], frac, &mut got);
            close(&got, &want);
        }
    }

    #[test]
    fn a_blur_block_matches_the_plain_convolution() {
        let x = line(80);
        let blur = Blur::new(2.4);
        let mut got = vec![0.0; 37];
        blur.apply(&x, 0.5, &mut got);
        let row = blur.row(0.5);
        let want: Vec<f32> = (0..37)
            .map(|k| row.iter().zip(&x[k..]).map(|(t, v)| t * v).sum())
            .collect();
        close(&got, &want);
    }
}

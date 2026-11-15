use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::{RealDecimator, design_lowpass_blackman_harris, fast_arg};

use super::{
    chroma::Alternation,
    standard::{SECAM_CENTRE_HZ, SECAM_DB_HZ, SECAM_DR_HZ},
};

const TAPS: usize = 25;
/// The filter keeps every other sample: the chroma band is far narrower than half the rate.
const DECIMATION: usize = 2;
/// Where the turn between filtered samples `m` and `m + 1` sits, in input samples past `2 m`.
const TURN_AT: f32 = (TAPS / 2 + 1) as f32;
const BAND_HZ: f64 = 900_000.0;
const DR_DEVIATION_HZ: f64 = 280_000.0;
const DB_DEVIATION_HZ: f64 = 230_000.0;
/// `D'r = -1.902 (R - Y)` and `D'b = 1.505 (B - Y)`, taken to U and V.
const DR_TO_V: f32 = -0.877 / 1.902;
const DB_TO_U: f32 = 0.493 / 1.505;
/// The corners of SECAM's low-frequency pre-emphasis, undone here.
const EMPHASIS_LOW_HZ: f64 = 85_000.0;
const EMPHASIS_HIGH_HZ: f64 = 3.0 * EMPHASIS_LOW_HZ;
/// Subcarrier amplitude, as a share of black to white, below which colour is killed.
const COLOUR_ON: f32 = 0.04;
const COLOUR_OFF: f32 = 0.025;
const AMPLITUDE_TRACK: f32 = 0.05;

/// The colour difference a SECAM line carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Difference {
    Red,
    Blue,
}

impl Difference {
    fn rest_hz(self) -> f64 {
        match self {
            Self::Red => SECAM_DR_HZ,
            Self::Blue => SECAM_DB_HZ,
        }
    }

    fn deviation_hz(self) -> f64 {
        match self {
            Self::Red => DR_DEVIATION_HZ,
            Self::Blue => DB_DEVIATION_HZ,
        }
    }
}

/// SECAM's FM chroma: each line carries one colour difference, told apart by the rest
/// frequency on its back porch.
#[derive(Clone, Debug)]
pub(super) struct Secam {
    rate: f64,
    /// The low-pass folded with the mixer for each start phase: taps on samples of `from`'s
    /// parity make the real part, the others the imaginary; reversed, so filtering the
    /// deinterleaved samples correlates.
    mixed: [(RealDecimator, RealDecimator); 4],
    halves: [Vec<f32>; 2],
    parts: [Vec<f32>; 2],
    deemphasis: [f32; 3],
    lines: Alternation,
    amplitude: f32,
    on: bool,
    filtered: Vec<Complex<f32>>,
    levels: Vec<f32>,
}

impl Secam {
    pub fn new(rate: f64) -> Self {
        Self {
            rate,
            mixed: mixed_taps(&design_lowpass_blackman_harris(TAPS, BAND_HZ / rate)),
            deemphasis: deemphasis(rate / DECIMATION as f64),
            lines: Alternation::new(),
            amplitude: 0.0,
            on: false,
            filtered: Vec::new(),
            levels: Vec::new(),
            halves: [Vec::new(), Vec::new()],
            parts: [Vec::new(), Vec::new()],
        }
    }

    /// Samples the filter reads past each end of the stretch it demodulates.
    pub fn reach(&self) -> usize {
        TAPS / 2 + 2 * DECIMATION
    }

    /// Reads the back porch's rest frequency, offset from the centre, and its amplitude;
    /// `raw[0]` is sample `from`.
    pub fn observe(&mut self, raw: &[f32], from: u64, span: f32) {
        self.analytic(raw, from);
        let Some(count) = self.filtered.len().checked_sub(1).filter(|&n| n > 0) else {
            return;
        };
        let mean_hz = self.turns().sum::<f32>() / count as f32;
        let level = self.filtered.iter().map(|z| z.norm()).sum::<f32>()
            / self.filtered.len() as f32
            / span.max(1e-6);
        self.amplitude += AMPLITUDE_TRACK * (level - self.amplitude);
        self.on = self.amplitude > if self.on { COLOUR_OFF } else { COLOUR_ON };
        let half_gap = 0.5 * (SECAM_DR_HZ - SECAM_DB_HZ) as f32;
        let swing = (mean_hz / half_gap).clamp(-1.0, 1.0);
        self.lines.step(swing);
    }

    pub fn coast(&mut self) {
        self.lines.coast();
    }

    /// The difference the last observed line carries, or none with colour killed.
    pub fn difference(&self) -> Option<Difference> {
        self.on.then(|| {
            if self.lines.sign() > 0.0 {
                Difference::Red
            } else {
                Difference::Blue
            }
        })
    }

    /// U or V, by `difference`, into `out`: `raw[0]` is sample `from`, `out[0]` sits on
    /// `raw[start]`, and `start` reaches back to a back porch at rest.
    pub fn demodulate(
        &mut self,
        raw: &[f32],
        from: u64,
        start: usize,
        difference: Difference,
        out: &mut [f32],
    ) {
        self.analytic(raw, from);
        let offset = (SECAM_CENTRE_HZ - difference.rest_hz()) as f32;
        let scale = (1.0 / difference.deviation_hz()) as f32
            * match difference {
                Difference::Red => DR_TO_V,
                Difference::Blue => DB_TO_U,
            };
        let [b0, b1, a1] = self.deemphasis;
        let mut levels = std::mem::take(&mut self.levels);
        levels.clear();
        let mut state: Option<(f32, f32)> = None;
        levels.extend(self.turns().map(|hz| {
            let x = (hz + offset) * scale;
            let (x1, y1) = state.unwrap_or((x, x));
            let y = b0 * x + b1 * x1 - a1 * y1;
            state = Some((x, y));
            y
        }));
        for (i, slot) in out.iter_mut().enumerate() {
            let at = ((start + i) as f32 - TURN_AT).max(0.0) / DECIMATION as f32;
            let m = at as usize;
            let frac = at - m as f32;
            let (Some(&a), Some(&b)) = (levels.get(m), levels.get(m + 1)) else {
                *slot = levels.last().copied().unwrap_or(0.0);
                continue;
            };
            *slot = a + frac * (b - a);
        }
        self.levels = levels;
    }

    /// The chroma band brought to baseband by a quarter-rate mixer, which is only signs, and
    /// low-passed at every other sample: `filtered[m]` sums `raw[2 m..2 m + TAPS]`.
    fn analytic(&mut self, raw: &[f32], from: u64) {
        self.filtered.clear();
        let Some(count) = raw.len().checked_sub(TAPS).map(|n| n / DECIMATION + 1) else {
            return;
        };
        for (parity, half) in self.halves.iter_mut().enumerate() {
            half.clear();
            half.extend(raw.iter().skip(parity).step_by(DECIMATION));
        }
        let parity = (from % 2) as usize;
        let (real, imaginary) = &mut self.mixed[(from % 4) as usize];
        let [re_part, im_part] = &mut self.parts;
        real.reset();
        real.process(&self.halves[parity], re_part);
        imaginary.reset();
        imaginary.process(&self.halves[1 - parity], im_part);
        let skip = |part: &[f32], taps: usize| part.len().min(taps.saturating_sub(1));
        let re = &self.parts[0][skip(&self.parts[0], TAPS.div_ceil(2) - parity)..];
        let im = &self.parts[1][skip(&self.parts[1], TAPS / 2 + parity)..];
        self.filtered.extend(
            re.iter()
                .zip(im)
                .take(count)
                .enumerate()
                .map(|(m, (&re, &im))| {
                    let sum = Complex::new(re, im);
                    if m % 2 == 1 { -sum } else { sum }
                }),
        );
    }

    /// Frequency, in Hz from the SECAM centre, between each filtered sample and the next.
    fn turns(&self) -> impl Iterator<Item = f32> + '_ {
        let to_hz = (self.rate / (TAU * DECIMATION as f64)) as f32;
        let quarter_rate = (self.rate / 4.0 - SECAM_CENTRE_HZ) as f32;
        self.filtered
            .windows(2)
            .map(move |pair| fast_arg(pair[1] * pair[0].conj()) * to_hz + quarter_rate)
    }
}

/// `taps` times the quarter-rate mixer from each start phase, split by the samples they meet.
fn mixed_taps(taps: &[f32]) -> [(RealDecimator, RealDecimator); 4] {
    std::array::from_fn(|phase| {
        let parity = phase % 2;
        let pick = |first: usize, part: fn(Complex<f32>) -> f32| {
            let picked: Vec<f32> = (first..taps.len())
                .step_by(2)
                .map(|k| part(quarter((phase + k) as u64)) * taps[k])
                .rev()
                .collect();
            RealDecimator::new(&picked, 1)
        };
        (pick(parity, |z| z.re), pick(1 - parity, |z| z.im))
    })
}

/// `e^{-j pi n / 2}`: the quarter-rate mixer at sample `n`.
fn quarter(n: u64) -> Complex<f32> {
    match n % 4 {
        0 => Complex::new(1.0, 0.0),
        1 => Complex::new(0.0, -1.0),
        2 => Complex::new(-1.0, 0.0),
        _ => Complex::new(0.0, 1.0),
    }
}

/// `b0, b1, a1` of the first-order shelf `(1 + s/ω_high) / (1 + s/ω_low)`, by the bilinear
/// transform.
fn deemphasis(rate: f64) -> [f32; 3] {
    let k = 2.0 * rate;
    let (low, high) = (TAU * EMPHASIS_LOW_HZ, TAU * EMPHASIS_HIGH_HZ);
    let a0 = 1.0 + k / low;
    [
        ((1.0 + k / high) / a0) as f32,
        ((1.0 - k / high) / a0) as f32,
        ((1.0 - k / low) / a0) as f32,
    ]
}

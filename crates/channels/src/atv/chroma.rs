use std::f32::consts::{FRAC_PI_2, TAU};

use num_complex::Complex;
use sdrmm_dsp::{design_lowpass_kaiser, madd};

const COLOUR_ON: f32 = 0.05;
const COLOUR_OFF: f32 = 0.03;
const PHASE_GAIN: f32 = 0.2;
const DRIFT_GAIN: f32 = 0.01;
const AMPLITUDE_TRACK: f32 = 0.05;
const IDENT_TRACK: f32 = 0.3;
/// Band-passed difference between the lines above and below, as a share of black to white,
/// at which the picture no longer counts as the same over three lines.
const STILL_RANGE: f32 = 0.3;
/// Low-passed difference between a line and a neighbour, as a share of black to white, at
/// which the neighbour no longer counts as the same picture.
const LUMA_RANGE: f32 = 0.15;

/// A line-by-line alternation, held by a flywheel through noise and lines without burst:
/// PAL's V switch, SECAM's red and blue lines.
#[derive(Clone, Debug)]
pub(super) struct Alternation {
    sign: f32,
    ident: f32,
}

impl Alternation {
    pub fn new() -> Self {
        Self {
            sign: 1.0,
            ident: 0.0,
        }
    }

    pub fn sign(&self) -> f32 {
        self.sign
    }

    /// The next line, with `swing` saying how strongly it reads as `+1` or `-1`.
    pub fn step(&mut self, swing: f32) {
        let expected = -self.sign;
        self.ident += IDENT_TRACK * (swing * expected - self.ident);
        self.sign = if self.ident < 0.0 {
            self.ident = -self.ident;
            -expected
        } else {
            expected
        };
    }

    pub fn coast(&mut self) {
        self.sign = -self.sign;
    }
}

/// Burst-locked subcarrier phase, the PAL switch and the colour killer.
#[derive(Clone, Debug)]
pub(super) struct Burst {
    pal: bool,
    theta: f32,
    drift: f32,
    previous: Complex<f32>,
    amplitude: f32,
    on: bool,
    switch: Alternation,
}

impl Burst {
    pub fn new(pal: bool) -> Self {
        Self {
            pal,
            theta: 0.0,
            drift: 0.0,
            previous: Complex::new(0.0, 0.0),
            amplitude: 0.0,
            on: false,
            switch: Alternation::new(),
        }
    }

    /// The rotation that puts the reference burst on +V - jU's imaginary axis, so
    /// `V = re` and `U = -im`.
    fn rotation(&self) -> Complex<f32> {
        Complex::from_polar(1.0, FRAC_PI_2 - self.theta)
    }

    pub fn update(&mut self, burst: Complex<f32>) {
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
        self.theta = (self.theta + self.drift).rem_euclid(TAU);
        let coherent = (pair * self.rotation()).im;
        self.amplitude += AMPLITUDE_TRACK * (coherent - self.amplitude);
        self.on = self.amplitude > if self.on { COLOUR_OFF } else { COLOUR_ON };
        if self.pal {
            self.switch.step((burst * self.rotation()).re);
        }
    }

    /// A line without burst: the phase runs on and PAL keeps alternating.
    pub fn coast(&mut self) {
        self.theta = (self.theta + self.drift).rem_euclid(TAU);
        if self.pal {
            self.switch.coast();
        }
    }

    /// This line's demodulation, for a line whose black to white spans `span`.
    pub fn turn(&self, span: f32) -> Turn {
        let rotation = self.rotation() / span;
        let scale = if self.on { 1.0 } else { 0.0 };
        let v_sign = if self.pal { self.switch.sign() } else { 1.0 };
        Turn {
            u: rotation * -scale,
            v: rotation * (v_sign * scale),
        }
    }
}

/// The burst rotation, scaled to U and V and by the colour killer and PAL switch.
#[derive(Clone, Copy, Debug)]
pub(super) struct Turn {
    pub u: Complex<f32>,
    pub v: Complex<f32>,
}

/// U and V of one line from the carrier-signed chroma, `carrier[k + 3]` under pixel `k`.
pub(super) fn demodulate_line(
    carrier: &[f32],
    first: u64,
    turn: Turn,
    (u, v): (&mut [f32], &mut [f32]),
) {
    let taps = (
        carrier
            .iter()
            .zip(&carrier[1..])
            .zip(&carrier[2..])
            .zip(&carrier[3..]),
        carrier[4..].iter().zip(&carrier[5..]).zip(&carrier[6..]),
    );
    let sums = taps
        .0
        .zip(taps.1)
        .map(|((((&c0, &c1), &c2), &c3), ((&c4, &c5), &c6))| {
            (
                0.25 * (c1 + 2.0 * c3 + c5),
                0.125 * (c0 + 3.0 * (c2 + c4) + c6),
            )
        });
    for (k, ((u, v), (same, other))) in u.iter_mut().zip(v.iter_mut()).zip(sums).enumerate() {
        let odd = (first + k as u64) & 1 == 1;
        let (i, q) = if odd { (other, same) } else { (same, other) };
        *u = i * turn.u.im + q * turn.u.re;
        *v = i * turn.v.re - q * turn.v.im;
    }
}

/// `+1, -1, -1, +1` by sample index: the I and Q carriers of a 4x subcarrier grid, merged
/// since only one is non-zero at a time.
fn sign(index: u64) -> f32 {
    if (index + 1) & 2 == 0 { 1.0 } else { -1.0 }
}

pub(super) fn modulate(raw: &[f32], from: u64, carrier: &mut Vec<f32>) {
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
pub(super) fn split(carrier: &[f32], even: &mut Vec<f32>, odd: &mut Vec<f32>) {
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
pub(super) fn phasor(index: u64, even: f32, odd: f32) -> Complex<f32> {
    if index & 1 == 0 {
        Complex::new(2.0 * even, 2.0 * odd)
    } else {
        Complex::new(2.0 * odd, 2.0 * even)
    }
}

/// Splits chroma from a composite line with the lines above and below, whose subcarrier
/// stands in opposite phase: where the neighbours match, their difference is chroma alone and
/// luma keeps its full width; where neither does, it falls back to a band-pass.
#[derive(Clone, Debug)]
pub(super) struct Comb {
    /// The band-pass's taps two samples apart; the ones between are zero at a quarter rate.
    band: Vec<f32>,
    line: Vec<f32>,
    up: Vec<f32>,
    down: Vec<f32>,
}

/// Samples the comb reads past each end of the stretch it separates: half its band-pass.
pub(super) const COMB_PAD: usize = 12;
/// Half the chroma band the band-pass keeps around the subcarrier; narrower than PAL's
/// 1.3 MHz so luma up to 2.5 MHz stays luma where the comb cannot help.
const BAND_HZ: f64 = 1_200_000.0;
const BAND_STOP_DB: f64 = 60.0;
/// The band as a share of the rate, held legal for monochrome rates the comb never runs at.
const MAX_BAND: f64 = 0.25;

impl Comb {
    /// A comb for lines sampled at `rate`, four times the subcarrier.
    pub fn new(rate: f64) -> Self {
        let low = design_lowpass_kaiser(
            2 * COMB_PAD + 1,
            (BAND_HZ / rate).min(MAX_BAND),
            BAND_STOP_DB,
        );
        Self {
            band: low
                .iter()
                .step_by(2)
                .enumerate()
                .map(|(j, &t)| {
                    let sign = if (j + COMB_PAD / 2).is_multiple_of(2) {
                        2.0
                    } else {
                        -2.0
                    };
                    sign * t
                })
                .collect(),
            line: Vec::new(),
            up: Vec::new(),
            down: Vec::new(),
        }
    }

    /// Chroma of `x` into `chroma`, `COMB_PAD` narrower than `x` at each end; `up` and
    /// `down` are the same stretch of the neighbours, `span` the line's black to white.
    pub fn separate(
        &mut self,
        x: &[f32],
        up: Option<&[f32]>,
        down: Option<&[f32]>,
        span: f32,
        chroma: &mut Vec<f32>,
    ) {
        let len = x.len().saturating_sub(2 * COMB_PAD);
        band_pass(&self.band, x, &mut self.line);
        chroma.clear();
        chroma.resize(len, 0.0);
        let ranges = Ranges {
            luma: 1.0 / (LUMA_RANGE * span),
            still: 1.0 / (STILL_RANGE * span),
        };
        let x = &x[COMB_PAD..COMB_PAD + len];
        match (up, down) {
            (Some(up), Some(down)) => {
                band_pass(&self.band, up, &mut self.up);
                band_pass(&self.band, down, &mut self.down);
                let near = (
                    &up[COMB_PAD..],
                    &self.up[..],
                    &down[COMB_PAD..],
                    &self.down[..],
                );
                centred(x, &self.line, near, ranges, chroma);
            }
            (Some(near), None) | (None, Some(near)) => {
                band_pass(&self.band, near, &mut self.up);
                one_sided(x, &self.line, (&near[COMB_PAD..], &self.up), ranges, chroma);
            }
            (None, None) => chroma.copy_from_slice(&self.line[..len]),
        }
    }
}

/// Reciprocal ranges of the comb's likeness tests.
#[derive(Clone, Copy)]
struct Ranges {
    luma: f32,
    still: f32,
}

/// How far a neighbour's low-passed luma sits from this line's, as a weight from 1 to 0.
#[inline(always)]
fn luma_likeness(raw: f32, band: f32, near_raw: f32, near_band: f32, inverse: f32) -> f32 {
    let gap = ((raw - band) - (near_raw - near_band)).abs();
    (1.0 - gap * inverse).clamp(0.0, 1.0)
}

/// Each side weighted by its likeness, and where the picture holds still over all three
/// lines, the centred comb `(2 x - up - down) / 4`.
fn centred(
    x: &[f32],
    line: &[f32],
    (up_raw, up, down_raw, down): (&[f32], &[f32], &[f32], &[f32]),
    ranges: Ranges,
    chroma: &mut [f32],
) {
    let mut previous = (up[0] - down[0]).abs();
    for (k, slot) in chroma.iter_mut().enumerate() {
        let (here, raw) = (line[k], x[k]);
        let above = luma_likeness(raw, here, up_raw[k], up[k], ranges.luma);
        let below = luma_likeness(raw, here, down_raw[k], down[k], ranges.luma);
        let (from_up, from_down) = (0.5 * (here - up[k]), 0.5 * (here - down[k]));
        let total = above + below;
        let comb = (above * from_up + below * from_down) / total.max(f32::MIN_POSITIVE);
        let mix = total.min(1.0);
        let sides = mix * comb + (1.0 - mix) * here;
        let parted = (up[k] - down[k]).abs();
        let still = (1.0 - (parted + previous) * ranges.still)
            .clamp(0.0, 1.0)
            .min(above)
            .min(below);
        previous = parted;
        *slot = still * 0.5 * (from_up + from_down) + (1.0 - still) * sides;
    }
}

/// One neighbour, weighted by its likeness, the band-pass making up the rest.
fn one_sided(
    x: &[f32],
    line: &[f32],
    (near_raw, near): (&[f32], &[f32]),
    ranges: Ranges,
    chroma: &mut [f32],
) {
    for (k, slot) in chroma.iter_mut().enumerate() {
        let (here, raw) = (line[k], x[k]);
        let weight = luma_likeness(raw, here, near_raw[k], near[k], ranges.luma);
        *slot = weight * 0.5 * (here - near[k]) + (1.0 - weight) * here;
    }
}

/// `x` through the band-pass whose every other tap is `band`: `out[k]` is centred on
/// `x[k + COMB_PAD]`.
fn band_pass(band: &[f32], x: &[f32], out: &mut Vec<f32>) {
    let len = x.len().saturating_sub(2 * COMB_PAD);
    out.clear();
    out.resize(len, 0.0);
    for (j, &tap) in band.iter().enumerate() {
        for (o, &v) in out.iter_mut().zip(&x[2 * j..]) {
            *o = madd(tap, v, *o);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn line_demodulation_reads_the_split_halves_by_sample_parity() {
        let raw = line(120);
        let (from, first, width) = (5u64, 12u64, 90usize);
        let (mut carrier, mut even, mut odd) = (vec![], vec![], vec![]);
        modulate(&raw, from, &mut carrier);
        split(&carrier, &mut even, &mut odd);
        let offset = (first - from) as usize;
        let identity = Turn {
            u: Complex::new(0.0, 1.0),
            v: Complex::new(0.0, -1.0),
        };
        let (mut u, mut v) = (vec![0.0; width], vec![0.0; width]);
        demodulate_line(&carrier[offset - 3..], first, identity, (&mut u, &mut v));
        let want: Vec<Complex<f32>> = (0..width)
            .map(|k| phasor(first + k as u64, even[offset + k], odd[offset + k]))
            .collect();
        close(&u, &want.iter().map(|z| z.re).collect::<Vec<_>>());
        close(&v, &want.iter().map(|z| z.im).collect::<Vec<_>>());
    }

    /// Fine luma detail plus a subcarrier that turns over between lines.
    fn composite(len: usize, chroma_sign: f32) -> (Vec<f32>, Vec<f32>) {
        let luma: Vec<f32> = (0..len)
            .map(|k| 0.5 + 0.3 * (TAU * 0.23 * k as f32).sin())
            .collect();
        let chroma: Vec<f32> = (0..len)
            .map(|k| chroma_sign * 0.2 * (TAU * 0.25 * k as f32 + 0.7).sin())
            .collect();
        let x = luma.iter().zip(&chroma).map(|(y, c)| y + c).collect();
        (x, chroma)
    }

    fn worst(a: &[f32], b: &[f32]) -> f32 {
        a.iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f32::max)
    }

    #[test]
    fn the_comb_keeps_luma_detail_out_of_chroma() {
        let (x, want) = composite(200, 1.0);
        let (neighbour, _) = composite(200, -1.0);
        let mut comb = Comb::new(4.0 * crate::atv::standard::PAL_SUBCARRIER_HZ);
        let mut chroma = Vec::new();
        comb.separate(&x, Some(&neighbour), Some(&neighbour), 1.0, &mut chroma);
        let want = &want[COMB_PAD..COMB_PAD + chroma.len()];
        assert!(worst(&chroma, want) < 1e-4, "comb {}", worst(&chroma, want));
        comb.separate(&x, None, None, 1.0, &mut chroma);
        assert!(
            worst(&chroma, want) > 0.1,
            "a band-pass alone should let luma detail through"
        );
    }

    #[test]
    fn the_comb_turns_from_a_neighbour_that_differs() {
        let (x, want) = composite(200, 1.0);
        let (above, _) = composite(200, -1.0);
        let below = vec![0.0; 200];
        let mut comb = Comb::new(4.0 * crate::atv::standard::PAL_SUBCARRIER_HZ);
        let mut chroma = Vec::new();
        comb.separate(&x, Some(&above), Some(&below), 1.0, &mut chroma);
        let want = &want[COMB_PAD..COMB_PAD + chroma.len()];
        assert!(worst(&chroma, want) < 1e-4, "comb {}", worst(&chroma, want));
    }

    #[test]
    fn the_alternation_follows_the_lines_and_rides_through_a_wrong_one() {
        let mut alternation = Alternation::new();
        let mut signs = Vec::new();
        for swing in [-1.0, 1.0, -1.0, 1.0, 1.0, 1.0, -1.0] {
            alternation.step(swing);
            signs.push(alternation.sign());
        }
        assert_eq!(signs, [-1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0]);
    }

    #[test]
    fn the_band_pass_keeps_chroma_and_most_luma_detail() {
        let rate = 4.0 * crate::atv::standard::PAL_SUBCARRIER_HZ;
        let comb = Comb::new(rate);
        let gain = |hz: f64| {
            let tone: Vec<f32> = (0..400)
                .map(|k| (std::f64::consts::TAU * hz / rate * f64::from(k)).sin() as f32)
                .collect();
            let mut out = Vec::new();
            band_pass(&comb.band, &tone, &mut out);
            out.iter().fold(0.0f32, |peak, v| peak.max(v.abs()))
        };
        assert!(
            (gain(rate / 4.0) - 1.0).abs() < 0.02,
            "subcarrier {}",
            gain(rate / 4.0)
        );
        assert!(
            gain(2_500_000.0) < 0.15,
            "2.5 MHz luma {}",
            gain(2_500_000.0)
        );
        assert!(gain(100_000.0) < 0.01, "low luma {}", gain(100_000.0));
    }
}

//! Wiener interpolation across frequency from pilots on a regular grid, designed for a
//! measured delay span: the first path, the spread, and the pilot noise.

use std::f64::consts::{PI, TAU};

use num_complex::Complex;
use sdrmm_dsp::math::fft::Transform;

pub const TAPS: usize = 8;
const MAX_SPACING: usize = 24;
const PATH_FLOOR: f32 = 1.0 / 16.0;
const PRECURSOR_SHARE: f32 = 0.1;
const CLASSES: usize = MAX_SPACING * (TAPS - 1) + 1;

#[derive(Clone, Copy, Debug, PartialEq)]
/// The pilot grid and channel a filter is designed for. Delays are in samples of an `fft`
/// long symbol.
pub struct Design {
    pub spacing: usize,
    pub fft: usize,
    pub first: f32,
    pub width: f32,
    pub noise: f32,
}

impl Design {
    fn quantised(self) -> Self {
        Self {
            first: self.first.round(),
            width: (self.width / 4.0).ceil() * 4.0,
            noise: 2f32.powf(self.noise.max(1e-6).log2().round()),
            ..self
        }
    }
}

pub struct FrequencyFilter {
    design: Option<Design>,
    weights_re: Box<[[f32; TAPS]; CLASSES]>,
    weights_im: Box<[[f32; TAPS]; CLASSES]>,
}

impl Default for FrequencyFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl FrequencyFilter {
    #[must_use]
    pub fn new() -> Self {
        Self {
            design: None,
            weights_re: Box::new([[0.0; TAPS]; CLASSES]),
            weights_im: Box::new([[0.0; TAPS]; CLASSES]),
        }
    }

    pub fn prepare(&mut self, design: Design) {
        let design = design.quantised();
        if self.design == Some(design) {
            return;
        }
        let gram = gram(design);
        let max_class = design.spacing * (TAPS - 1);
        for class in 0..=max_class.min(CLASSES - 1) {
            let w = unbiased(design, class, solve(&gram, &rhs(design, class)));
            for i in 0..TAPS {
                self.weights_re[class][i] = w[i].re;
                self.weights_im[class][i] = w[i].im;
            }
        }
        self.design = Some(design);
    }

    /// The channel at carrier `k` from `count` pilots, the first at carrier `first`.
    #[must_use]
    pub fn at(
        &self,
        anchors: &impl Fn(usize) -> Complex<f32>,
        first: usize,
        count: usize,
        k: usize,
    ) -> Complex<f32> {
        let Some(design) = self.design else {
            return Complex::new(0.0, 0.0);
        };
        if count < TAPS {
            return Complex::new(0.0, 0.0);
        }
        let spacing = design.spacing;
        let nearest = k.saturating_sub(first) / spacing;
        let start = (nearest + 1).saturating_sub(TAPS / 2).min(count - TAPS);
        let class = (k as isize - (first + start * spacing) as isize)
            .clamp(0, (spacing * (TAPS - 1)) as isize) as usize;
        let mut ar = [0.0f32; TAPS];
        let mut ai = [0.0f32; TAPS];
        for i in 0..TAPS {
            let a = anchors(start + i);
            ar[i] = a.re;
            ai[i] = a.im;
        }
        dot8(&self.weights_re[class], &self.weights_im[class], &ar, &ai)
    }

    pub fn apply(
        &self,
        anchors: impl Fn(usize) -> Complex<f32>,
        first: usize,
        count: usize,
        out: &mut [Complex<f32>],
    ) {
        let Some(design) = self.design else {
            out.fill(Complex::new(0.0, 0.0));
            return;
        };
        if count < TAPS {
            out.fill(Complex::new(0.0, 0.0));
            return;
        }
        let spacing = design.spacing;
        let max_start = count - TAPS;
        let max_class = spacing * (TAPS - 1);
        let mut start = 0usize;
        let mut ar = [0.0f32; TAPS];
        let mut ai = [0.0f32; TAPS];
        for i in 0..TAPS {
            let a = anchors(i);
            ar[i] = a.re;
            ai[i] = a.im;
        }
        let mut nearest = 0usize;
        let mut rem = 0usize;
        for (k, slot) in out.iter_mut().enumerate() {
            if k >= first {
                if k > first {
                    rem += 1;
                    if rem == spacing {
                        rem = 0;
                        nearest += 1;
                    }
                }
                let next_start = (nearest + 1).saturating_sub(TAPS / 2).min(max_start);
                if next_start != start {
                    if next_start == start + 1 {
                        ar.copy_within(1.., 0);
                        ai.copy_within(1.., 0);
                        let a = anchors(next_start + TAPS - 1);
                        ar[TAPS - 1] = a.re;
                        ai[TAPS - 1] = a.im;
                    } else {
                        for i in 0..TAPS {
                            let a = anchors(next_start + i);
                            ar[i] = a.re;
                            ai[i] = a.im;
                        }
                    }
                    start = next_start;
                }
            }
            let base = first + start * spacing;
            let class = if k <= base {
                0
            } else {
                (k - base).min(max_class)
            };
            *slot = dot8(&self.weights_re[class], &self.weights_im[class], &ar, &ai);
        }
    }
}

#[cfg(all(target_arch = "x86_64", target_feature = "avx", target_feature = "fma"))]
#[inline(always)]
fn dot8(wr: &[f32; TAPS], wi: &[f32; TAPS], ar: &[f32; TAPS], ai: &[f32; TAPS]) -> Complex<f32> {
    use std::arch::x86_64::*;
    unsafe {
        let wr = _mm256_loadu_ps(wr.as_ptr());
        let wi = _mm256_loadu_ps(wi.as_ptr());
        let ar = _mm256_loadu_ps(ar.as_ptr());
        let ai = _mm256_loadu_ps(ai.as_ptr());
        let re = _mm256_fmsub_ps(wr, ar, _mm256_mul_ps(wi, ai));
        let im = _mm256_fmadd_ps(wr, ai, _mm256_mul_ps(wi, ar));
        let re = _mm_add_ps(_mm256_castps256_ps128(re), _mm256_extractf128_ps::<1>(re));
        let im = _mm_add_ps(_mm256_castps256_ps128(im), _mm256_extractf128_ps::<1>(im));
        let sums = _mm_hadd_ps(re, im);
        let sums = _mm_hadd_ps(sums, sums);
        Complex::new(
            _mm_cvtss_f32(sums),
            _mm_cvtss_f32(_mm_shuffle_ps::<0x55>(sums, sums)),
        )
    }
}

#[cfg(not(all(target_arch = "x86_64", target_feature = "avx", target_feature = "fma")))]
#[inline(always)]
fn dot8(wr: &[f32; TAPS], wi: &[f32; TAPS], ar: &[f32; TAPS], ai: &[f32; TAPS]) -> Complex<f32> {
    let mut re = 0.0f32;
    let mut im = 0.0f32;
    for i in 0..TAPS {
        re = sdrmm_dsp::madd(wr[i], ar[i], re);
        re = sdrmm_dsp::madd(-wi[i], ai[i], re);
        im = sdrmm_dsp::madd(wr[i], ai[i], im);
        im = sdrmm_dsp::madd(wi[i], ar[i], im);
    }
    Complex::new(re, im)
}

/// The first and last path delay of a pilot response, `None` for silence.
pub fn measure_span(
    profile: &mut [Complex<f32>],
    inverse: &mut Transform,
    values: impl Fn(usize) -> Complex<f32>,
    count: usize,
    period: f32,
    guard: usize,
) -> Option<(f32, f32)> {
    profile.fill(Complex::new(0.0, 0.0));
    if count > 0 {
        let step_angle = TAU / count as f64;
        let (sin_s, cos_s) = step_angle.sin_cos();
        let (sin_0, cos_0) = (0.5 * step_angle).sin_cos();
        let mut cr = cos_0;
        let mut ci = sin_0;
        for (m, slot) in profile.iter_mut().take(count).enumerate() {
            let taper = 0.5 - 0.5 * cr as f32;
            *slot = values(m) * taper;
            let next_r = cr * cos_s - ci * sin_s;
            ci = cr * sin_s + ci * cos_s;
            cr = next_r;
        }
    }
    inverse.process(profile);
    let strongest = profile.iter().map(Complex::norm_sqr).fold(0.0f32, f32::max);
    if !(strongest > 0.0 && strongest.is_finite()) {
        return None;
    }
    let len = profile.len();
    let per_index = period / len as f32;
    let early = ((period - guard as f32) / 2.0).max(period * PRECURSOR_SHARE);
    let first_late = (((period - early) / per_index).ceil() as usize).min(len);
    let delay = |j: usize| {
        let delay = j as f32 * per_index;
        if j >= first_late {
            delay - period
        } else {
            delay
        }
    };
    let strong = |j: &usize| profile[*j].norm_sqr() >= strongest * PATH_FLOOR;
    let first = (first_late..len).chain(0..first_late).find(strong)?;
    let last = (0..first_late)
        .rev()
        .chain((first_late..len).rev())
        .find(strong)?;
    Some((delay(first), delay(last).max(delay(first))))
}

fn correlation(design: Design, distance: f64) -> Complex<f64> {
    let width = f64::from(design.width.max(1.0));
    let centre = f64::from(design.first) + width / 2.0;
    let fft = design.fft as f64;
    let x = distance * width / fft;
    let sinc = if x.abs() < 1e-9 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    };
    Complex::from_polar(sinc, -TAU * distance * centre / fft)
}

fn gram(design: Design) -> [[Complex<f64>; TAPS]; TAPS] {
    let spacing = design.spacing as f64;
    std::array::from_fn(|row| {
        std::array::from_fn(|col| {
            let value = correlation(design, (col as f64 - row as f64) * spacing);
            if row == col {
                value + f64::from(design.noise)
            } else {
                value
            }
        })
    })
}

fn rhs(design: Design, class: usize) -> [Complex<f64>; TAPS] {
    std::array::from_fn(|i| correlation(design, class as f64 - (i * design.spacing) as f64))
}

fn unbiased(design: Design, class: usize, weights: [Complex<f32>; TAPS]) -> [Complex<f32>; TAPS] {
    let centre = f64::from(design.first) + f64::from(design.width.max(1.0)) / 2.0;
    let response: Complex<f64> = weights
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let distance = class as f64 - (i * design.spacing) as f64;
            Complex::new(f64::from(w.re), f64::from(w.im))
                * Complex::from_polar(1.0, TAU * distance * centre / design.fft as f64)
        })
        .sum();
    if response.norm() < 0.5 {
        return weights;
    }
    let gain = response.inv();
    weights.map(|w| {
        let scaled = Complex::new(f64::from(w.re), f64::from(w.im)) * gain;
        Complex::new(scaled.re as f32, scaled.im as f32)
    })
}

fn solve(gram: &[[Complex<f64>; TAPS]; TAPS], rhs: &[Complex<f64>; TAPS]) -> [Complex<f32>; TAPS] {
    let mut a: [[Complex<f64>; TAPS + 1]; TAPS] = std::array::from_fn(|row| {
        std::array::from_fn(|col| if col < TAPS { gram[row][col] } else { rhs[row] })
    });
    for pivot in 0..TAPS {
        let best = (pivot..TAPS)
            .max_by(|&x, &y| a[x][pivot].norm_sqr().total_cmp(&a[y][pivot].norm_sqr()))
            .unwrap_or(pivot);
        a.swap(pivot, best);
        let lead = a[pivot][pivot];
        if lead.norm_sqr() < 1e-300 {
            return [Complex::new(0.0, 0.0); TAPS];
        }
        for value in &mut a[pivot][pivot..] {
            *value /= lead;
        }
        let lead_row = a[pivot];
        for (row, values) in a.iter_mut().enumerate() {
            if row != pivot {
                let factor = values[pivot];
                for (value, &lead) in values[pivot..].iter_mut().zip(&lead_row[pivot..]) {
                    *value -= factor * lead;
                }
            }
        }
    }
    std::array::from_fn(|i| Complex::new(a[i][TAPS].re as f32, a[i][TAPS].im as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(paths: &[(f64, Complex<f64>)], k: usize, fft: usize) -> Complex<f32> {
        let value: Complex<f64> = paths
            .iter()
            .map(|&(delay, gain)| {
                gain * Complex::from_polar(1.0, -TAU * k as f64 * delay / fft as f64)
            })
            .sum();
        Complex::new(value.re as f32, value.im as f32)
    }

    fn worst_error(spacing: usize, paths: &[(f64, Complex<f64>)], first: f32, width: f32) -> f32 {
        let (fft, carriers) = (2048, 1705);
        let mut filter = FrequencyFilter::new();
        filter.prepare(Design {
            spacing,
            fft,
            first,
            width,
            noise: 1e-4,
        });
        let count = (carriers - 1) / spacing + 1;
        let mut out = vec![Complex::new(0.0, 0.0); carriers];
        filter.apply(|m| channel(paths, m * spacing, fft), 0, count, &mut out);
        out.iter()
            .enumerate()
            .skip(TAPS * spacing)
            .take(carriers - 2 * TAPS * spacing)
            .map(|(k, &h)| (h - channel(paths, k, fft)).norm())
            .fold(0.0, f32::max)
    }

    #[test]
    fn a_long_echo_is_followed_between_pilots_three_carriers_apart() {
        let paths = [
            (0.0, Complex::new(1.0, 0.0)),
            (300.0, Complex::new(0.0, 0.9)),
        ];
        let worst = worst_error(3, &paths, -4.0, 312.0);
        assert!(worst < 0.05, "worst error {worst}");
    }

    #[test]
    fn a_short_channel_is_followed_on_the_sparse_grid() {
        let paths = [
            (0.0, Complex::new(0.8, 0.2)),
            (40.0, Complex::new(-0.4, 0.1)),
        ];
        let worst = worst_error(12, &paths, -4.0, 52.0);
        assert!(worst < 0.05, "worst error {worst}");
    }

    #[test]
    fn the_edges_stay_close_to_the_channel() {
        let (fft, carriers, spacing) = (2048, 1705, 3);
        let paths = [(5.0, Complex::new(1.0, 0.0))];
        let mut filter = FrequencyFilter::new();
        filter.prepare(Design {
            spacing,
            fft,
            first: 0.0,
            width: 16.0,
            noise: 1e-4,
        });
        let mut out = vec![Complex::new(0.0, 0.0); carriers];
        filter.apply(
            |m| channel(&paths, m * spacing, fft),
            0,
            (carriers - 1) / spacing + 1,
            &mut out,
        );
        for k in [0, 1, 2, carriers - 2, carriers - 1] {
            let error = (out[k] - channel(&paths, k, fft)).norm();
            assert!(error < 0.05, "carrier {k}: error {error}");
        }
    }
}

use num_complex::Complex;

use crate::filter::fir::StreamFir;

#[derive(Clone, Debug)]
enum Phase<T> {
    Zero,
    Tap {
        delay: usize,
        gain: f32,
        ring: Vec<T>,
        cursor: usize,
    },
    Fir(StreamFir<T, f32>),
}

impl<T> Phase<T>
where
    T: Copy + Default + std::ops::Mul<f32, Output = T> + crate::filter::fir::Accumulate<f32>,
{
    fn new(taps: &[f32]) -> Self {
        let mut nonzero = taps.iter().enumerate().filter(|&(_, &tap)| tap != 0.0);
        match (nonzero.next(), nonzero.next()) {
            (None, _) => Self::Zero,
            (Some((delay, &gain)), None) => Self::Tap {
                delay,
                gain,
                ring: vec![T::default(); delay],
                cursor: 0,
            },
            _ => Self::Fir(StreamFir::new(taps, 1)),
        }
    }

    fn reset(&mut self) {
        match self {
            Self::Zero => {}
            Self::Tap { ring, cursor, .. } => {
                ring.fill(T::default());
                *cursor = 0;
            }
            Self::Fir(fir) => fir.reset(),
        }
    }

    fn process(&mut self, input: &[T], out: &mut Vec<T>) {
        match self {
            Self::Zero => {
                out.clear();
                out.resize(input.len(), T::default());
            }
            Self::Tap {
                delay,
                gain,
                ring,
                cursor,
            } => {
                out.clear();
                out.reserve(input.len());
                let g = *gain;
                let d = *delay;
                if d == 0 {
                    if g == 1.0 {
                        out.extend_from_slice(input);
                    } else {
                        out.extend(input.iter().map(|&x| x * g));
                    }
                    return;
                }
                let n = input.len();
                let mut c = *cursor;
                if n >= d {
                    let tail = d - c;
                    if g == 1.0 {
                        out.extend_from_slice(&ring[c..]);
                        out.extend_from_slice(&ring[..c]);
                        out.extend_from_slice(&input[..n - d]);
                    } else {
                        out.extend(ring[c..].iter().map(|&x| x * g));
                        out.extend(ring[..c].iter().map(|&x| x * g));
                        out.extend(input[..n - d].iter().map(|&x| x * g));
                    }
                    let keep = &input[n - d..];
                    ring[c..].copy_from_slice(&keep[..tail]);
                    ring[..c].copy_from_slice(&keep[tail..]);
                } else {
                    for &x in input {
                        let y = ring[c];
                        ring[c] = x;
                        c += 1;
                        if c == d {
                            c = 0;
                        }
                        out.push(if g == 1.0 { y } else { y * g });
                    }
                    *cursor = c;
                }
            }
            Self::Fir(fir) => fir.process(input, out),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RealInterpolator {
    phases: Vec<Phase<f32>>,
    scratch: Vec<Vec<f32>>,
}

impl RealInterpolator {
    #[must_use]
    pub fn new(taps: &[f32], factor: usize) -> Self {
        let phases: Vec<_> = split_phases(taps, factor)
            .into_iter()
            .map(|row| Phase::new(&row))
            .collect();
        let scratch = (0..phases.len()).map(|_| Vec::new()).collect();
        Self { phases, scratch }
    }

    pub fn reset(&mut self) {
        for phase in &mut self.phases {
            phase.reset();
        }
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        out.clear();
        let factor = self.phases.len();
        let n = input.len();
        out.reserve(n * factor);
        for (phase, buf) in self.phases.iter_mut().zip(&mut self.scratch) {
            phase.process(input, buf);
        }
        interleave(&self.scratch, n, out);
    }
}

/// The complex counterpart of [`RealInterpolator`]: zero-stuffs by `factor` and filters the images.
#[derive(Clone, Debug)]
pub struct Interpolator {
    phases: Vec<Phase<Complex<f32>>>,
    scratch: Vec<Vec<Complex<f32>>>,
}

impl Interpolator {
    #[must_use]
    pub fn new(taps: &[f32], factor: usize) -> Self {
        let phases: Vec<_> = split_phases(taps, factor)
            .into_iter()
            .map(|row| Phase::new(&row))
            .collect();
        let scratch = (0..phases.len()).map(|_| Vec::new()).collect();
        Self { phases, scratch }
    }

    pub fn reset(&mut self) {
        for phase in &mut self.phases {
            phase.reset();
        }
    }

    pub fn process(&mut self, input: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        out.clear();
        let factor = self.phases.len();
        let n = input.len();
        out.reserve(n * factor);
        for (phase, buf) in self.phases.iter_mut().zip(&mut self.scratch) {
            phase.process(input, buf);
        }
        interleave(&self.scratch, n, out);
    }
}

fn interleave<T: Copy>(lanes: &[Vec<T>], len: usize, out: &mut Vec<T>) {
    match lanes {
        [a, b] => {
            let base = out.len();
            let dst = out.spare_capacity_mut();
            for (i, (&x, &y)) in a[..len].iter().zip(&b[..len]).enumerate() {
                dst[2 * i].write(x);
                dst[2 * i + 1].write(y);
            }
            unsafe { out.set_len(base + 2 * len) };
        }
        [a, b, c] => {
            let base = out.len();
            let dst = out.spare_capacity_mut();
            for i in 0..len {
                dst[3 * i].write(a[i]);
                dst[3 * i + 1].write(b[i]);
                dst[3 * i + 2].write(c[i]);
            }
            unsafe { out.set_len(base + 3 * len) };
        }
        _ => {
            for i in 0..len {
                for lane in lanes {
                    out.push(lane[i]);
                }
            }
        }
    }
}

/// The `factor` phases of `taps`, zero-padded to one length and scaled by `factor` so the
/// zero-stuffed stream keeps its level.
fn split_phases(taps: &[f32], factor: usize) -> Vec<Vec<f32>> {
    assert!(factor >= 1, "factor must be >= 1");
    assert!(taps.len() >= factor, "need at least one tap per phase");
    let per_phase = taps.len().div_ceil(factor);
    let gain = factor as f32;
    (0..factor)
        .map(|phase| {
            (0..per_phase)
                .map(|k| taps.get(phase + k * factor).copied().unwrap_or(0.0) * gain)
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        design_lowpass,
        filter::fir::design_lowpass_kaiser,
        testutil::{complex_tone, real_tone, rms_r},
    };

    #[test]
    fn a_real_interpolator_keeps_an_in_band_tone_and_its_level() {
        let taps = design_lowpass(96, 0.15);
        let mut up = RealInterpolator::new(&taps, 3);
        let input = real_tone(0.05, 4_000);
        let mut out = Vec::new();
        up.process(&input, &mut out);
        assert_eq!(out.len(), 3 * input.len());
        let level = rms_r(&out[600..]);
        assert!((0.68..0.73).contains(&level), "interpolated rms {level}");
    }

    #[test]
    fn a_real_interpolator_suppresses_the_images_it_makes() {
        let taps = design_lowpass(96, 0.15);
        let mut up = RealInterpolator::new(&taps, 3);
        let input = real_tone(0.1, 6_000);
        let mut out = Vec::new();
        up.process(&input, &mut out);
        let settled = &out[600..600 + 4_096];
        let image = settled
            .iter()
            .enumerate()
            .map(|(n, &x)| {
                let p = std::f64::consts::TAU * (1.0 / 3.0 - 0.1 / 3.0) * n as f64;
                Complex::new(x * p.cos() as f32, -x * p.sin() as f32)
            })
            .sum::<Complex<f32>>()
            .norm()
            / settled.len() as f32;
        assert!(image < 1e-3, "image leaked at {image}");
    }

    #[test]
    fn a_complex_interpolator_keeps_the_tone_and_stops_its_image() {
        let taps = design_lowpass_kaiser(63, 0.25, 80.0);
        let mut up = Interpolator::new(&taps, 2);
        let input = complex_tone(0.05, 6_000);
        let mut out = Vec::new();
        up.process(&input, &mut out);
        assert_eq!(out.len(), 2 * input.len());
        let settled = &out[200..200 + 4_096];
        let level = |turns: f64| {
            settled
                .iter()
                .enumerate()
                .map(|(n, x)| {
                    let p = -std::f64::consts::TAU * turns * n as f64;
                    x * Complex::new(p.cos() as f32, p.sin() as f32)
                })
                .sum::<Complex<f32>>()
                .norm()
                / settled.len() as f32
        };
        assert!(
            (level(0.025) - 1.0).abs() < 1e-2,
            "tone at {}",
            level(0.025)
        );
        assert!(level(0.525) < 1e-3, "image at {}", level(0.525));
    }

    #[test]
    fn a_complex_interpolator_gives_the_same_result_in_ragged_blocks() {
        let taps = design_lowpass_kaiser(31, 0.25, 80.0);
        let input = complex_tone(0.07, 3_000);
        let mut whole = Interpolator::new(&taps, 2);
        let mut expected = Vec::new();
        whole.process(&input, &mut expected);
        let mut ragged = Interpolator::new(&taps, 2);
        let mut got = Vec::new();
        let mut block = Vec::new();
        for chunk in input.chunks(97) {
            ragged.process(chunk, &mut block);
            got.extend_from_slice(&block);
        }
        assert_eq!(expected, got);
    }

    #[test]
    fn a_real_interpolator_gives_the_same_result_in_ragged_blocks() {
        let taps = design_lowpass(96, 0.15);
        let input = real_tone(0.07, 3_000);
        let mut whole = RealInterpolator::new(&taps, 3);
        let mut expected = Vec::new();
        whole.process(&input, &mut expected);
        let mut ragged = RealInterpolator::new(&taps, 3);
        let mut got = Vec::new();
        let mut block = Vec::new();
        for chunk in input.chunks(97) {
            ragged.process(chunk, &mut block);
            got.extend_from_slice(&block);
        }
        assert_eq!(expected, got);
    }
}

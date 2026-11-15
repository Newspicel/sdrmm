use std::ops::Mul;

use super::*;
use crate::{filter::fir::scalar, math::complex::widen};

fn complex_ramp(len: usize, a: usize, b: usize) -> Vec<Complex<f32>> {
    (0..len)
        .map(|index| {
            Complex::new(
                (index % a) as f32 / a as f32 - 0.5,
                (index % b) as f32 / b as f32 - 0.5,
            )
        })
        .collect()
}

trait Widen: Copy {
    fn widened(self) -> Complex<f64>;
}

impl Widen for f32 {
    fn widened(self) -> Complex<f64> {
        Complex::new(f64::from(self), 0.0)
    }
}

impl Widen for Complex<f32> {
    fn widened(self) -> Complex<f64> {
        widen(self)
    }
}

fn lengths() -> impl Iterator<Item = usize> {
    (1..=17).chain([31, 32, 33, 63, 64, 65, 127, 128, 129, 257])
}

fn one_output<T, C>(isa: Isa, window: &[T], taps: &[C], fold: bool) -> T
where
    T: Accumulate<C>,
    C: Copy + PartialEq + Default + Mul<f32, Output = C>,
{
    let plan = Plan::new::<T>(taps, 1, window.len() + T::LANES, fold);
    let mut padded = window.to_vec();
    padded.resize(plan.reach + (GROUP_BLOCKS - 1) * T::LANES, T::zero());
    let mut out = [T::zero()];
    if plan.folded() {
        T::blocks::<true>(isa, &padded, &plan, &mut out);
    } else {
        T::blocks::<false>(isa, &padded, &plan, &mut out);
    }
    out[0]
}

#[test]
fn batched_products_match_double_precision_for_every_tail_length() {
    for isa in Isa::available() {
        for len in 1..=257 {
            let samples = complex_ramp(len, 7, 11);
            let taps = complex_ramp(len, 13, 17);
            let reference: Complex<f64> = samples
                .iter()
                .zip(taps.iter().rev())
                .map(|(&sample, &tap)| widen(sample) * widen(tap))
                .sum();
            let actual = widen(one_output(isa, &samples, &taps, false));
            assert!(
                (actual - reference).norm() < len as f64 * 1e-7,
                "{isa:?} length={len}"
            );
            let real_samples: Vec<_> = samples.iter().map(|sample| sample.re).collect();
            let real_taps: Vec<_> = taps.iter().map(|tap| tap.re).collect();
            let reference: Complex<f64> = samples
                .iter()
                .zip(real_taps.iter().rev())
                .map(|(&sample, &tap)| widen(sample) * f64::from(tap))
                .sum();
            let actual = widen(one_output(isa, &samples, &real_taps, false));
            assert!(
                (actual - reference).norm() < len as f64 * 1e-7,
                "{isa:?} real taps, length={len}"
            );
            let reference: f64 = real_samples
                .iter()
                .zip(real_taps.iter().rev())
                .map(|(&sample, &tap)| f64::from(sample) * f64::from(tap))
                .sum();
            let actual = f64::from(one_output(isa, &real_samples, &real_taps, false));
            assert!(
                (actual - reference).abs() < len as f64 * 1e-7,
                "{isa:?} real, length={len}"
            );
        }
    }
}

fn check_block<T, C>(isa: Isa, window: &[T], taps: &[C], phases: usize, label: &str)
where
    T: Accumulate<C> + Widen + std::fmt::Debug,
    C: Copy + PartialEq + Default + Mul<f32, Output = C>,
{
    let stride = window.len().div_ceil(phases);
    for fold in [false, true] {
        let plan = Plan::new::<T>(taps, phases, stride, fold);
        let mut padded = window.to_vec();
        padded.resize(
            plan.reach.max(window.len()) + (GROUP_BLOCKS - 1) * T::LANES,
            T::zero(),
        );
        let mut simd = vec![T::zero(); T::LANES];
        let mut reference = simd.clone();
        if plan.folded() {
            T::blocks::<true>(isa, &padded, &plan, &mut simd);
            scalar::block::<T, C, true>(&padded[..plan.reach], &plan, &mut reference);
        } else {
            T::blocks::<false>(isa, &padded, &plan, &mut simd);
            scalar::block::<T, C, false>(&padded[..plan.reach], &plan, &mut reference);
        }
        for (lane, (&a, &b)) in simd.iter().zip(&reference).enumerate() {
            let error = (a.widened() - b.widened()).norm();
            assert!(
                error <= 1e-6 * (taps.len() as f64 + 1.0),
                "{isa:?} {label} taps={} phases={phases} fold={fold} lane={lane}: {a:?} vs {b:?}",
                taps.len()
            );
        }
    }
}

#[test]
fn every_block_kernel_matches_the_scalar_reference_at_every_length() {
    for isa in Isa::available() {
        for len in lengths() {
            for phases in [1, 2, 3, 7] {
                let window = complex_ramp(4 * (len + 64), 7, 11);
                let real_window: Vec<f32> = window.iter().map(|sample| sample.im).collect();
                let complex_taps = complex_ramp(len, 13, 17);
                let real_taps: Vec<f32> = complex_taps.iter().map(|tap| tap.re).collect();
                let symmetric: Vec<f32> = (0..len)
                    .map(|index| real_taps[index.min(len - 1 - index)])
                    .collect();
                let symmetric_complex: Vec<_> = (0..len)
                    .map(|index| complex_taps[index.min(len - 1 - index)])
                    .collect();
                check_block(isa, &real_window, &real_taps, phases, "real");
                check_block(isa, &real_window, &symmetric, phases, "real symmetric");
                check_block(isa, &window, &real_taps, phases, "complex real");
                check_block(isa, &window, &symmetric, phases, "complex symmetric");
                check_block(isa, &window, &complex_taps, phases, "complex");
                check_block(
                    isa,
                    &window,
                    &symmetric_complex,
                    phases,
                    "complex taps symmetric",
                );
            }
        }
    }
}

fn planes(len: usize) -> (Vec<f32>, Vec<f32>, Vec<f32>, Vec<f32>) {
    let samples = complex_ramp(len, 7, 11);
    let taps = complex_ramp(len, 13, 17);
    (
        samples.iter().map(|sample| sample.re).collect(),
        samples.iter().map(|sample| sample.im).collect(),
        taps.iter().map(|tap| tap.re).collect(),
        taps.iter().map(|tap| tap.im * 0.1).collect(),
    )
}

fn planar_reference(re: &[f32], im: &[f32], taps: impl Iterator<Item = f64>) -> Complex<f64> {
    re.iter()
        .zip(im)
        .zip(taps)
        .map(|((&re, &im), tap)| Complex::new(f64::from(re), f64::from(im)) * tap)
        .sum()
}

#[test]
fn planar_dot_matches_double_precision_and_is_the_same_for_any_batch() {
    for isa in Isa::available() {
        for len in (0..=17).chain(lengths()) {
            let (re, im, taps, _) = planes(len + 9);
            let taps = &taps[..len];
            let starts = [0, 3, 9, 1];
            let batch = plane_dot(isa, &re, &im, starts, taps);
            for (&start, &actual) in starts.iter().zip(&batch) {
                let window = start..start + len;
                let reference = planar_reference(
                    &re[window.clone()],
                    &im[window],
                    taps.iter().map(|&tap| f64::from(tap)),
                );
                assert!(
                    (widen(actual) - reference).norm() <= 1e-6 * (len as f64 + 1.0),
                    "{isa:?} length={len} start={start}"
                );
                assert_eq!(
                    plane_dot(isa, &re, &im, [start], taps)[0],
                    actual,
                    "{isa:?} length={len} start={start}"
                );
            }
        }
    }
}

#[test]
fn planar_interpolation_matches_double_precision_at_every_length() {
    for isa in Isa::available() {
        for len in (0..=17).chain(lengths()) {
            let (re, im, lower, slope) = planes(len);
            let mu = 0.625;
            let reference = planar_reference(
                &re,
                &im,
                lower
                    .iter()
                    .zip(&slope)
                    .map(|(&lower, &slope)| f64::from(lower) + f64::from(slope) * f64::from(mu)),
            );
            let actual = widen(plane_interpolated(isa, &re, &im, &lower, &slope, mu));
            assert!(
                (actual - reference).norm() <= 1e-6 * (len as f64 + 1.0),
                "{isa:?} length={len}: {actual} vs {reference}"
            );
        }
    }
}

#[test]
fn complex_taps_accumulate_the_full_product() {
    let sum = Complex::new(0.25f32, -0.5);
    let sample = Complex::new(0.75f32, -1.25);
    let tap = Complex::new(-0.5f32, 2.0);
    assert_eq!(sum.add_product(sample, tap), sum + sample * tap);
}

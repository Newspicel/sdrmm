use num_complex::Complex;

use super::atan::fast_arg;
#[cfg(target_arch = "x86_64")]
use super::avx;
#[cfg(target_arch = "aarch64")]
use super::neon;

pub fn phase_diff_into(
    iq: &[Complex<f32>],
    prev: &mut Complex<f32>,
    scale: f32,
    out: &mut Vec<f32>,
) {
    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("avx2") {
        return unsafe { phase_diff_avx2(iq, prev, scale, out) };
    }
    phase_diff_body(iq, prev, scale, out);
}

/// The same loop compiled eight lanes wide.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn phase_diff_avx2(iq: &[Complex<f32>], prev: &mut Complex<f32>, scale: f32, out: &mut Vec<f32>) {
    phase_diff_body(iq, prev, scale, out);
}

#[inline(always)]
fn phase_diff_body(iq: &[Complex<f32>], prev: &mut Complex<f32>, scale: f32, out: &mut Vec<f32>) {
    out.clear();
    let (Some(&first), Some(&last)) = (iq.first(), iq.last()) else {
        return;
    };
    let len = iq.len();
    out.reserve(len);
    let dst = out.spare_capacity_mut().as_mut_ptr().cast::<f32>();
    unsafe {
        dst.write(fast_arg(first * prev.conj()) * scale);
        let done = vector_phase_diffs(iq, scale, dst.add(1), len - 1);
        for k in done..len - 1 {
            let value = fast_arg(iq[k + 1] * iq[k].conj()) * scale;
            dst.add(1 + k).write(value);
        }
        out.set_len(len);
    }
    *prev = last;
}

#[cfg(target_arch = "x86_64")]
unsafe fn vector_phase_diffs(iq: &[Complex<f32>], scale: f32, dst: *mut f32, len: usize) -> usize {
    if std::arch::is_x86_feature_detected!("avx2") {
        let slice = unsafe { std::slice::from_raw_parts_mut(dst, len) };
        unsafe { avx::phase_diffs(iq, scale, slice) }
    } else {
        0
    }
}

#[cfg(target_arch = "aarch64")]
unsafe fn vector_phase_diffs(iq: &[Complex<f32>], scale: f32, dst: *mut f32, len: usize) -> usize {
    unsafe { neon::phase_diffs(iq, scale, dst, len) }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
unsafe fn vector_phase_diffs(_: &[Complex<f32>], _: f32, _: *mut f32, _: usize) -> usize {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_differences_match_the_scalar_formula_bit_for_bit() {
        let mut iq: Vec<Complex<f32>> = (0..203)
            .map(|k| {
                let k = k as f32;
                Complex::new((k * 0.37).sin() * (1.0 + k), (k * 0.91).cos() - 0.5)
            })
            .collect();
        iq[17] = Complex::new(0.0, 0.0);
        iq[40] = Complex::new(-2.0, 2.0);
        iq[41] = Complex::new(3.0, -0.0);
        let scale = 0.731;
        let mut prev = Complex::new(0.25, -1.0);
        let mut out = Vec::new();
        phase_diff_into(&iq, &mut prev, scale, &mut out);
        let mut before = Complex::new(0.25, -1.0);
        for (k, (&now, &got)) in iq.iter().zip(&out).enumerate() {
            let expected = fast_arg(now * before.conj()) * scale;
            assert_eq!(
                got.to_bits(),
                expected.to_bits(),
                "sample {k}: {got} vs {expected}"
            );
            before = now;
        }
        assert_eq!(prev, iq[iq.len() - 1]);
    }

    #[test]
    fn block_matches_scalar_per_sample() {
        let iq: Vec<Complex<f32>> = (0..1_031)
            .map(|k| {
                let k = k as f32;
                Complex::from_polar(0.1 + (k * 0.37).sin().abs(), k * 0.91 + (k * 0.05).cos())
            })
            .collect();
        let start = Complex::new(0.3, -0.8);
        let (scale, mut prev, mut out) = (1.7, start, Vec::new());
        phase_diff_into(&iq, &mut prev, scale, &mut out);
        assert_eq!(out.len(), iq.len());
        assert_eq!(prev, iq[iq.len() - 1]);
        let mut before = start;
        for (&x, &got) in iq.iter().zip(&out) {
            assert_eq!(
                got.to_bits(),
                (fast_arg(x * before.conj()) * scale).to_bits()
            );
            before = x;
        }
    }

    #[test]
    fn block_carries_state_across_calls_and_ignores_empty_input() {
        let iq: Vec<Complex<f32>> = (0..64)
            .map(|k| Complex::from_polar(1.0, 0.2 * k as f32))
            .collect();
        let (mut whole, mut split) = (Vec::new(), Vec::new());
        let mut prev = Complex::new(1.0, 0.0);
        phase_diff_into(&iq, &mut prev, 1.0, &mut whole);
        let mut prev = Complex::new(1.0, 0.0);
        let mut part = Vec::new();
        for chunk in iq.chunks(7) {
            phase_diff_into(&[], &mut prev, 1.0, &mut part);
            assert!(part.is_empty());
            phase_diff_into(chunk, &mut prev, 1.0, &mut part);
            split.extend_from_slice(&part);
        }
        assert_eq!(whole, split);
        assert!(whole[1..].iter().all(|&v| (v - 0.2).abs() < 1e-5));
    }
}

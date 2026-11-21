use num_complex::Complex;

use super::{MANTISSA_BITS, madd};

const SQRT_HALF_BITS: u32 = 0x3F35_04F3;
const SUBNORMAL_SCALE: f32 = 16_777_216.0;
const SUBNORMAL_OCTAVES: f32 = 24.0;
const LOG2_SERIES: [f32; 6] = [
    1.442_713_3,
    -0.721_132_04,
    0.479_351_46,
    -0.367_487_73,
    0.322_139_23,
    -0.206_597_67,
];
const POWER_DB_PER_OCTAVE: f32 = 10.0 * std::f32::consts::LOG10_2;

#[inline(always)]
fn log2_series(t: f32) -> f32 {
    let [c0, c1, c2, c3, c4, c5] = LOG2_SERIES;
    let p01 = madd(t, c1, c0);
    let p23 = madd(t, c3, c2);
    let p45 = madd(t, c5, c4);
    let t2 = t * t;
    madd(t2, madd(t2, p45, p23), p01)
}

#[must_use]
#[inline(always)]
pub fn fast_log2(x: f32) -> f32 {
    let subnormal = x < f32::MIN_POSITIVE;
    let scaled = if subnormal { x * SUBNORMAL_SCALE } else { x };
    let bits = scaled.to_bits();
    let exponent = (bits.wrapping_sub(SQRT_HALF_BITS) as i32) >> MANTISSA_BITS;
    let mantissa = f32::from_bits(bits.wrapping_sub((exponent as u32) << MANTISSA_BITS));
    let t = mantissa - 1.0;
    let octaves = exponent as f32 - if subnormal { SUBNORMAL_OCTAVES } else { 0.0 };
    let log = madd(t, log2_series(t), octaves);
    let log = if x == f32::INFINITY { x } else { log };
    if x > 0.0 {
        log
    } else if x == 0.0 {
        f32::NEG_INFINITY
    } else {
        f32::NAN
    }
}

/// Fast `10 * log10(power)` for normal positive finite inputs (`power >= f32::MIN_POSITIVE`).
#[must_use]
#[inline(always)]
pub fn fast_power_db_normal(power: f32) -> f32 {
    let bits = power.to_bits();
    let exponent = (bits.wrapping_sub(SQRT_HALF_BITS) as i32) >> MANTISSA_BITS;
    let mantissa = f32::from_bits(bits.wrapping_sub((exponent as u32) << MANTISSA_BITS));
    let t = mantissa - 1.0;
    madd(t, log2_series(t), exponent as f32) * POWER_DB_PER_OCTAVE
}

#[must_use]
#[inline(always)]
pub fn fast_log10(x: f32) -> f32 {
    fast_log2(x) * std::f32::consts::LOG10_2
}

#[must_use]
#[inline(always)]
pub fn fast_power_db(power: f32) -> f32 {
    fast_log2(power) * POWER_DB_PER_OCTAVE
}

/// Computes `10 * log10(bin.norm_sqr() * scale + epsilon)` for each bin, assuming `epsilon >= f32::MIN_POSITIVE`.
pub fn power_db_bins(bins: &[Complex<f32>], scale: f32, epsilon: f32, out: &mut [f32]) {
    let len = bins.len().min(out.len());
    let bins = &bins[..len];
    let out = &mut out[..len];
    #[cfg(target_arch = "aarch64")]
    {
        power_db_bins_neon(bins, scale, epsilon, out);
    }
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma")
        {
            unsafe { power_db_bins_avx2(bins, scale, epsilon, out) };
        } else {
            power_db_bins_scalar(bins, scale, epsilon, out);
        }
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        power_db_bins_scalar(bins, scale, epsilon, out);
    }
}

fn power_db_bins_scalar(bins: &[Complex<f32>], scale: f32, epsilon: f32, out: &mut [f32]) {
    for (dst, bin) in out.iter_mut().zip(bins) {
        let p = madd(bin.re, bin.re, bin.im * bin.im);
        *dst = fast_power_db_normal(madd(p, scale, epsilon));
    }
}

#[cfg(target_arch = "aarch64")]
struct NeonLogConsts {
    scale: std::arch::aarch64::float32x4_t,
    eps: std::arch::aarch64::float32x4_t,
    sqrt_half: std::arch::aarch64::uint32x4_t,
    one: std::arch::aarch64::float32x4_t,
    c: [std::arch::aarch64::float32x4_t; 6],
    db: std::arch::aarch64::float32x4_t,
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn eval4_neon(
    re: std::arch::aarch64::float32x4_t,
    im: std::arch::aarch64::float32x4_t,
    k: &NeonLogConsts,
) -> std::arch::aarch64::float32x4_t {
    use std::arch::aarch64::{
        vcvtq_f32_s32, vfmaq_f32, vmulq_f32, vreinterpretq_f32_u32, vreinterpretq_s32_u32,
        vreinterpretq_u32_f32, vreinterpretq_u32_s32, vshlq_n_u32, vshrq_n_s32, vsubq_f32,
        vsubq_u32,
    };
    unsafe {
        let p = vfmaq_f32(vmulq_f32(im, im), re, re);
        let x = vfmaq_f32(k.eps, p, k.scale);
        let bits = vreinterpretq_u32_f32(x);
        let exp = vshrq_n_s32::<23>(vreinterpretq_s32_u32(vsubq_u32(bits, k.sqrt_half)));
        let mantissa = vreinterpretq_f32_u32(vsubq_u32(
            bits,
            vshlq_n_u32::<23>(vreinterpretq_u32_s32(exp)),
        ));
        let t = vsubq_f32(mantissa, k.one);
        let t2 = vmulq_f32(t, t);
        let p01 = vfmaq_f32(k.c[0], t, k.c[1]);
        let p23 = vfmaq_f32(k.c[2], t, k.c[3]);
        let p45 = vfmaq_f32(k.c[4], t, k.c[5]);
        let poly = vfmaq_f32(p01, t2, vfmaq_f32(p23, t2, p45));
        let log2 = vfmaq_f32(vcvtq_f32_s32(exp), t, poly);
        vmulq_f32(log2, k.db)
    }
}

#[cfg(target_arch = "aarch64")]
fn power_db_bins_neon(bins: &[Complex<f32>], scale: f32, epsilon: f32, out: &mut [f32]) {
    use std::arch::aarch64::{vdupq_n_f32, vdupq_n_u32, vld2q_f32, vst1q_f32};
    let (out_blocks, out_tail) = out.as_chunks_mut::<8>();
    let (bin_blocks, bin_tail) = bins.as_chunks::<8>();
    unsafe {
        let k = NeonLogConsts {
            scale: vdupq_n_f32(scale),
            eps: vdupq_n_f32(epsilon),
            sqrt_half: vdupq_n_u32(SQRT_HALF_BITS),
            one: vdupq_n_f32(1.0),
            c: LOG2_SERIES.map(|v| vdupq_n_f32(v)),
            db: vdupq_n_f32(POWER_DB_PER_OCTAVE),
        };
        for (dst, src) in out_blocks.iter_mut().zip(bin_blocks) {
            let sp = src.as_ptr().cast::<f32>();
            let v0 = vld2q_f32(sp);
            let v1 = vld2q_f32(sp.add(8));
            vst1q_f32(dst.as_mut_ptr(), eval4_neon(v0.0, v0.1, &k));
            vst1q_f32(dst.as_mut_ptr().add(4), eval4_neon(v1.0, v1.1, &k));
        }
        let (out_q, out_rem) = out_tail.as_chunks_mut::<4>();
        let (bin_q, bin_rem) = bin_tail.as_chunks::<4>();
        for (dst, src) in out_q.iter_mut().zip(bin_q) {
            let v = vld2q_f32(src.as_ptr().cast());
            vst1q_f32(dst.as_mut_ptr(), eval4_neon(v.0, v.1, &k));
        }
        power_db_bins_scalar(bin_rem, scale, epsilon, out_rem);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "fma")]
unsafe fn power_db_bins_avx2(bins: &[Complex<f32>], scale: f32, epsilon: f32, out: &mut [f32]) {
    use std::arch::x86_64::{
        _mm256_castpd_ps, _mm256_castps_pd, _mm256_castps_si256, _mm256_castsi256_ps,
        _mm256_cvtepi32_ps, _mm256_fmadd_ps, _mm256_loadu_ps, _mm256_mul_ps, _mm256_permute4x64_pd,
        _mm256_set1_epi32, _mm256_set1_ps, _mm256_shuffle_ps, _mm256_slli_epi32, _mm256_srai_epi32,
        _mm256_storeu_ps, _mm256_sub_epi32, _mm256_sub_ps,
    };
    let (out_blocks, out_tail) = out.as_chunks_mut::<8>();
    let (bin_blocks, bin_tail) = bins.as_chunks::<8>();
    let vscale = _mm256_set1_ps(scale);
    let veps = _mm256_set1_ps(epsilon);
    let vsqrt_half = _mm256_set1_epi32(SQRT_HALF_BITS as i32);
    let vone = _mm256_set1_ps(1.0);
    let vc0 = _mm256_set1_ps(LOG2_SERIES[0]);
    let vc1 = _mm256_set1_ps(LOG2_SERIES[1]);
    let vc2 = _mm256_set1_ps(LOG2_SERIES[2]);
    let vc3 = _mm256_set1_ps(LOG2_SERIES[3]);
    let vc4 = _mm256_set1_ps(LOG2_SERIES[4]);
    let vc5 = _mm256_set1_ps(LOG2_SERIES[5]);
    let vdb = _mm256_set1_ps(POWER_DB_PER_OCTAVE);
    for (dst, src) in out_blocks.iter_mut().zip(bin_blocks) {
        let sp = src.as_ptr().cast::<f32>();
        let lo = unsafe { _mm256_loadu_ps(sp) };
        let hi = unsafe { _mm256_loadu_ps(sp.add(8)) };
        let re = _mm256_shuffle_ps::<0x88>(lo, hi);
        let im = _mm256_shuffle_ps::<0xDD>(lo, hi);
        let p = _mm256_fmadd_ps(re, re, _mm256_mul_ps(im, im));
        let x = _mm256_fmadd_ps(p, vscale, veps);
        let bits = _mm256_castps_si256(x);
        let exp = _mm256_srai_epi32::<23>(_mm256_sub_epi32(bits, vsqrt_half));
        let mantissa = _mm256_castsi256_ps(_mm256_sub_epi32(bits, _mm256_slli_epi32::<23>(exp)));
        let t = _mm256_sub_ps(mantissa, vone);
        let t2 = _mm256_mul_ps(t, t);
        let p01 = _mm256_fmadd_ps(t, vc1, vc0);
        let p23 = _mm256_fmadd_ps(t, vc3, vc2);
        let p45 = _mm256_fmadd_ps(t, vc5, vc4);
        let poly = _mm256_fmadd_ps(t2, _mm256_fmadd_ps(t2, p45, p23), p01);
        let log2 = _mm256_fmadd_ps(t, poly, _mm256_cvtepi32_ps(exp));
        let db = _mm256_mul_ps(log2, vdb);
        let ordered = _mm256_castpd_ps(_mm256_permute4x64_pd::<0xD8>(_mm256_castps_pd(db)));
        unsafe { _mm256_storeu_ps(dst.as_mut_ptr(), ordered) };
    }
    power_db_bins_scalar(bin_tail, scale, epsilon, out_tail);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_positive_float(stride: u32) -> impl Iterator<Item = f32> {
        (1..f32::INFINITY.to_bits())
            .step_by(stride as usize)
            .chain([
                f32::MIN_POSITIVE.to_bits() - 1,
                f32::MIN_POSITIVE.to_bits(),
                f32::MAX.to_bits(),
            ])
            .map(f32::from_bits)
    }

    #[test]
    fn log10_stays_within_three_micro_decades_of_std_from_subnormals_to_max() {
        let worst = every_positive_float(997)
            .map(|x| (fast_log10(x) - x.log10()).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 3e-5, "worst {worst}");
    }

    #[test]
    fn log2_is_exact_at_powers_of_two_and_close_to_std_near_one() {
        for exponent in -149i32..128 {
            let x = if exponent < -126 {
                f32::from_bits(1 << (exponent + 149))
            } else {
                f32::from_bits(((exponent + 127) as u32) << 23)
            };
            assert!(
                (fast_log2(x) - exponent as f32).abs() < 1e-5,
                "2^{exponent}"
            );
        }
        let worst = (0..100_000)
            .map(|step| 0.5 + step as f32 / 50_000.0)
            .map(|x| (fast_log2(x) - x.log2()).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 5e-6, "worst {worst}");
    }

    #[test]
    fn power_db_is_invisible_next_to_std_on_a_display_scale() {
        let worst = every_positive_float(9_973)
            .map(|p| (fast_power_db(p) - 10.0 * p.log10()).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-3, "worst {worst} dB");
    }

    #[test]
    fn edge_values_match_std() {
        assert_eq!(fast_log10(0.0), f32::NEG_INFINITY);
        assert_eq!(fast_log10(-0.0), f32::NEG_INFINITY);
        assert_eq!(fast_log10(f32::INFINITY), f32::INFINITY);
        assert!(fast_log10(-1.0).is_nan());
        assert!(fast_log10(f32::NEG_INFINITY).is_nan());
        assert!(fast_log10(f32::NAN).is_nan());
        assert_eq!(fast_log10(1.0), 0.0);
    }

    #[test]
    fn power_db_bins_matches_scalar() {
        for len in [0, 1, 3, 4, 7, 8, 15, 65] {
            let bins: Vec<Complex<f32>> = (0..len)
                .map(|i| {
                    let s = 10.0f32.powf((i as f32 % 12.0) - 6.0);
                    Complex::new(
                        ((i * 19) % 23) as f32 * 0.1 * s,
                        ((i * 7) % 17) as f32 * 0.1 * s,
                    )
                })
                .collect();
            let scale = 0.25f32;
            let eps = 1e-24f32;
            let mut got = vec![0.0f32; len];
            let mut want = vec![0.0f32; len];
            power_db_bins(&bins, scale, eps, &mut got);
            power_db_bins_scalar(&bins, scale, eps, &mut want);
            for (k, (&g, &w)) in got.iter().zip(&want).enumerate() {
                assert!((g - w).abs() < 1e-4, "len={len} k={k}: {g} vs {w}");
            }
        }
    }
}

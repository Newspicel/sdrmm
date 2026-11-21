use std::arch::x86_64::{
    __m256, _CMP_EQ_OQ, _CMP_GT_OQ, _CMP_LT_OQ, _mm256_add_ps, _mm256_and_ps, _mm256_andnot_ps,
    _mm256_blendv_ps, _mm256_castpd_ps, _mm256_castps_pd, _mm256_cmp_ps, _mm256_div_ps,
    _mm256_loadu_ps, _mm256_mul_ps, _mm256_or_ps, _mm256_permute4x64_pd, _mm256_set1_ps,
    _mm256_setzero_ps, _mm256_shuffle_ps, _mm256_storeu_ps, _mm256_sub_ps,
};

use num_complex::Complex;

use super::atan::ATAN_UNIT;

const LANES: usize = 8;

/// Real and imaginary parts of eight samples, in the order `0 1 4 5 2 3 6 7`.
#[target_feature(enable = "avx2")]
unsafe fn split(samples: *const Complex<f32>) -> [__m256; 2] {
    let (low, high) = unsafe {
        (
            _mm256_loadu_ps(samples.cast()),
            _mm256_loadu_ps(samples.add(LANES / 2).cast()),
        )
    };
    [
        _mm256_shuffle_ps::<0x88>(low, high),
        _mm256_shuffle_ps::<0xDD>(low, high),
    ]
}

/// [`super::atan::fast_atan2`] on eight lanes, multiplying and adding without fusing so every lane
/// rounds exactly like the scalar version.
#[target_feature(enable = "avx2")]
fn atan2(y: __m256, x: __m256) -> __m256 {
    let sign = _mm256_set1_ps(-0.0);
    let (ax, ay) = (_mm256_andnot_ps(sign, x), _mm256_andnot_ps(sign, y));
    let steep = _mm256_cmp_ps::<_CMP_GT_OQ>(ay, ax);
    let lo = _mm256_blendv_ps(ay, ax, steep);
    let hi = _mm256_blendv_ps(ax, ay, steep);
    let level = _mm256_cmp_ps::<_CMP_EQ_OQ>(ax, ay);
    let ratio = _mm256_blendv_ps(_mm256_div_ps(lo, hi), _mm256_set1_ps(1.0), level);
    let square = _mm256_mul_ps(ratio, ratio);
    let s2 = _mm256_mul_ps(square, square);
    let s4 = _mm256_mul_ps(s2, s2);
    let p01 = _mm256_add_ps(
        _mm256_mul_ps(square, _mm256_set1_ps(ATAN_UNIT[1])),
        _mm256_set1_ps(ATAN_UNIT[0]),
    );
    let p23 = _mm256_add_ps(
        _mm256_mul_ps(square, _mm256_set1_ps(ATAN_UNIT[3])),
        _mm256_set1_ps(ATAN_UNIT[2]),
    );
    let p45 = _mm256_add_ps(
        _mm256_mul_ps(square, _mm256_set1_ps(ATAN_UNIT[5])),
        _mm256_set1_ps(ATAN_UNIT[4]),
    );
    let p03 = _mm256_add_ps(_mm256_mul_ps(s2, p23), p01);
    let p46 = _mm256_add_ps(_mm256_mul_ps(s2, _mm256_set1_ps(ATAN_UNIT[6])), p45);
    let mut angle = _mm256_mul_ps(_mm256_add_ps(_mm256_mul_ps(s4, p46), p03), ratio);
    let quarter = _mm256_sub_ps(_mm256_set1_ps(std::f32::consts::FRAC_PI_2), angle);
    angle = _mm256_blendv_ps(angle, quarter, steep);
    let behind = _mm256_cmp_ps::<_CMP_LT_OQ>(x, _mm256_setzero_ps());
    let half = _mm256_sub_ps(_mm256_set1_ps(std::f32::consts::PI), angle);
    angle = _mm256_blendv_ps(angle, half, behind);
    let origin = _mm256_cmp_ps::<_CMP_EQ_OQ>(_mm256_add_ps(ax, ay), _mm256_setzero_ps());
    angle = _mm256_blendv_ps(angle, _mm256_setzero_ps(), origin);
    _mm256_or_ps(_mm256_andnot_ps(sign, angle), _mm256_and_ps(sign, y))
}

/// Writes `arg(iq[k + 1] * conj(iq[k])) * scale` to `out[k]` for as many whole groups of eight
/// as fit, and returns how many it wrote.
#[target_feature(enable = "avx2")]
pub(super) unsafe fn phase_diffs(iq: &[Complex<f32>], scale: f32, out: &mut [f32]) -> usize {
    let count = out.len().min(iq.len().saturating_sub(1)) / LANES * LANES;
    let scale = _mm256_set1_ps(scale);
    let mut start = 0;
    while start + 2 * LANES <= count {
        let [b0_re, b0_im] = unsafe { split(iq.as_ptr().add(start)) };
        let [n0_re, n0_im] = unsafe { split(iq.as_ptr().add(start + 1)) };
        let [b1_re, b1_im] = unsafe { split(iq.as_ptr().add(start + LANES)) };
        let [n1_re, n1_im] = unsafe { split(iq.as_ptr().add(start + LANES + 1)) };
        let re0 = _mm256_add_ps(_mm256_mul_ps(n0_re, b0_re), _mm256_mul_ps(n0_im, b0_im));
        let im0 = _mm256_sub_ps(_mm256_mul_ps(n0_im, b0_re), _mm256_mul_ps(n0_re, b0_im));
        let re1 = _mm256_add_ps(_mm256_mul_ps(n1_re, b1_re), _mm256_mul_ps(n1_im, b1_im));
        let im1 = _mm256_sub_ps(_mm256_mul_ps(n1_im, b1_re), _mm256_mul_ps(n1_re, b1_im));
        let a0 = _mm256_mul_ps(atan2(im0, re0), scale);
        let a1 = _mm256_mul_ps(atan2(im1, re1), scale);
        let o0 = _mm256_permute4x64_pd::<0xD8>(_mm256_castps_pd(a0));
        let o1 = _mm256_permute4x64_pd::<0xD8>(_mm256_castps_pd(a1));
        unsafe {
            _mm256_storeu_ps(out.as_mut_ptr().add(start), _mm256_castpd_ps(o0));
            _mm256_storeu_ps(out.as_mut_ptr().add(start + LANES), _mm256_castpd_ps(o1));
        }
        start += 2 * LANES;
    }
    while start < count {
        let [before_re, before_im] = unsafe { split(iq.as_ptr().add(start)) };
        let [now_re, now_im] = unsafe { split(iq.as_ptr().add(start + 1)) };
        let re = _mm256_add_ps(
            _mm256_mul_ps(now_re, before_re),
            _mm256_mul_ps(now_im, before_im),
        );
        let im = _mm256_sub_ps(
            _mm256_mul_ps(now_im, before_re),
            _mm256_mul_ps(now_re, before_im),
        );
        let angles = _mm256_mul_ps(atan2(im, re), scale);
        let ordered = _mm256_permute4x64_pd::<0xD8>(_mm256_castps_pd(angles));
        unsafe { _mm256_storeu_ps(out.as_mut_ptr().add(start), _mm256_castpd_ps(ordered)) };
        start += LANES;
    }
    count
}

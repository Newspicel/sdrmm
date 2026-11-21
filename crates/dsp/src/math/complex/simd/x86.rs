use std::arch::x86_64::{
    __m128, __m256, __m256d, _mm_add_ps, _mm_cvtss_f32, _mm_loadu_ps, _mm_movehdup_ps,
    _mm_movehl_ps, _mm_unpackhi_ps, _mm_unpacklo_ps, _mm256_add_pd, _mm256_add_ps,
    _mm256_addsub_ps, _mm256_castpd_ps, _mm256_castps_pd, _mm256_castps256_ps128, _mm256_cvtps_pd,
    _mm256_extractf128_pd, _mm256_extractf128_ps, _mm256_fmadd_pd, _mm256_fmadd_ps,
    _mm256_fmaddsub_pd, _mm256_fmaddsub_ps, _mm256_fmsubadd_pd, _mm256_fmsubadd_ps, _mm256_hadd_ps,
    _mm256_loadu_pd, _mm256_loadu_ps, _mm256_movehdup_ps, _mm256_moveldup_ps, _mm256_mul_pd,
    _mm256_mul_ps, _mm256_permute_pd, _mm256_permute_ps, _mm256_permute4x64_pd, _mm256_set_m128,
    _mm256_set1_pd, _mm256_set1_ps, _mm256_setzero_pd, _mm256_setzero_ps, _mm256_sqrt_ps,
    _mm256_storeu_pd, _mm256_storeu_ps, _mm256_sub_ps, _mm256_unpackhi_pd, _mm256_unpacklo_pd,
};

use num_complex::Complex;

use super::{FOLD_CHUNK, scalar};

#[inline(always)]
fn has_avx2_fma() -> bool {
    is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma")
}

#[target_feature(enable = "avx2,fma")]
fn hsum_f32(v: __m256) -> f32 {
    let half = _mm_add_ps(_mm256_castps256_ps128(v), _mm256_extractf128_ps::<1>(v));
    let pair = _mm_add_ps(half, _mm_movehl_ps(half, half));
    _mm_cvtss_f32(_mm_add_ps(pair, _mm_movehdup_ps(pair)))
}

#[target_feature(enable = "avx2,fma")]
fn hsum_c32(v: __m256) -> Complex<f32> {
    let half = _mm_add_ps(_mm256_castps256_ps128(v), _mm256_extractf128_ps::<1>(v));
    let sum = _mm_add_ps(half, _mm_movehl_ps(half, half));
    let mut out = [0.0f32; 4];
    unsafe { std::arch::x86_64::_mm_storeu_ps(out.as_mut_ptr(), sum) };
    Complex::new(out[0], out[1])
}

#[target_feature(enable = "avx2,fma")]
fn hsum_c64(v: __m256d) -> Complex<f64> {
    let low = std::arch::x86_64::_mm256_castpd256_pd128(v);
    let high = _mm256_extractf128_pd::<1>(v);
    let sum = std::arch::x86_64::_mm_add_pd(low, high);
    let mut out = [0.0f64; 2];
    unsafe { std::arch::x86_64::_mm_storeu_pd(out.as_mut_ptr(), sum) };
    Complex::new(out[0], out[1])
}

#[target_feature(enable = "avx2,fma")]
fn dup_quad_f32(w: __m128) -> __m256 {
    _mm256_set_m128(_mm_unpackhi_ps(w, w), _mm_unpacklo_ps(w, w))
}

pub(super) fn cross_sum(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f64> {
    if has_avx2_fma() {
        return unsafe { cross_sum_avx2(left, right) };
    }
    scalar::cross_sum(left, right)
}

#[target_feature(enable = "avx2,fma")]
unsafe fn cross_sum_avx2(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f64> {
    let mut total = Complex::new(0.0f64, 0.0);
    for (a_chunk, b_chunk) in left.chunks(FOLD_CHUNK).zip(right.chunks(FOLD_CHUNK)) {
        total += unsafe { cross_sum_chunk_avx2(a_chunk, b_chunk) };
    }
    total
}

#[target_feature(enable = "avx2,fma")]
unsafe fn cross_sum_chunk_avx2(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f64> {
    let (a_blocks, a_rest) = left.as_chunks::<8>();
    let (b_blocks, b_rest) = right.as_chunks::<8>();
    let mut p0 = _mm256_setzero_ps();
    let mut q0 = _mm256_setzero_ps();
    let mut p1 = _mm256_setzero_ps();
    let mut q1 = _mm256_setzero_ps();
    for (a, b) in a_blocks.iter().zip(b_blocks) {
        let ap = a.as_ptr().cast::<f32>();
        let bp = b.as_ptr().cast::<f32>();
        let (va0, vb0) = unsafe { (_mm256_loadu_ps(ap), _mm256_loadu_ps(bp)) };
        let (va1, vb1) = unsafe { (_mm256_loadu_ps(ap.add(8)), _mm256_loadu_ps(bp.add(8))) };
        p0 = _mm256_fmadd_ps(va0, _mm256_moveldup_ps(vb0), p0);
        q0 = _mm256_fmadd_ps(va0, _mm256_movehdup_ps(vb0), q0);
        p1 = _mm256_fmadd_ps(va1, _mm256_moveldup_ps(vb1), p1);
        q1 = _mm256_fmadd_ps(va1, _mm256_movehdup_ps(vb1), q1);
    }
    let (a4, a_tail) = a_rest.as_chunks::<4>();
    let (b4, b_tail) = b_rest.as_chunks::<4>();
    for (a, b) in a4.iter().zip(b4) {
        let (va, vb) = unsafe {
            (
                _mm256_loadu_ps(a.as_ptr().cast()),
                _mm256_loadu_ps(b.as_ptr().cast()),
            )
        };
        p0 = _mm256_fmadd_ps(va, _mm256_moveldup_ps(vb), p0);
        q0 = _mm256_fmadd_ps(va, _mm256_movehdup_ps(vb), q0);
    }
    let p = _mm256_add_ps(p0, p1);
    let q = _mm256_permute_ps::<0xB1>(_mm256_add_ps(q0, q1));
    let combined = _mm256_fmsubadd_ps(p, _mm256_set1_ps(1.0), q);
    let s = hsum_c32(combined);
    let tail = scalar::cross_sum(a_tail, b_tail);
    Complex::new(f64::from(s.re) + tail.re, f64::from(s.im) + tail.im)
}

pub(super) fn conj_dot(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f32> {
    if has_avx2_fma() {
        return unsafe { conj_dot_avx2(left, right) };
    }
    scalar::conj_dot(left, right)
}

#[target_feature(enable = "avx2,fma")]
unsafe fn conj_dot_avx2(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f32> {
    let (a_blocks, a_rest) = left.as_chunks::<8>();
    let (b_blocks, b_rest) = right.as_chunks::<8>();
    let mut p0 = _mm256_setzero_ps();
    let mut q0 = _mm256_setzero_ps();
    let mut p1 = _mm256_setzero_ps();
    let mut q1 = _mm256_setzero_ps();
    for (a, b) in a_blocks.iter().zip(b_blocks) {
        let ap = a.as_ptr().cast::<f32>();
        let bp = b.as_ptr().cast::<f32>();
        let (va0, vb0) = unsafe { (_mm256_loadu_ps(ap), _mm256_loadu_ps(bp)) };
        let (va1, vb1) = unsafe { (_mm256_loadu_ps(ap.add(8)), _mm256_loadu_ps(bp.add(8))) };
        p0 = _mm256_fmadd_ps(vb0, _mm256_moveldup_ps(va0), p0);
        q0 = _mm256_fmadd_ps(vb0, _mm256_movehdup_ps(va0), q0);
        p1 = _mm256_fmadd_ps(vb1, _mm256_moveldup_ps(va1), p1);
        q1 = _mm256_fmadd_ps(vb1, _mm256_movehdup_ps(va1), q1);
    }
    let (a4, a_tail) = a_rest.as_chunks::<4>();
    let (b4, b_tail) = b_rest.as_chunks::<4>();
    for (a, b) in a4.iter().zip(b4) {
        let (va, vb) = unsafe {
            (
                _mm256_loadu_ps(a.as_ptr().cast()),
                _mm256_loadu_ps(b.as_ptr().cast()),
            )
        };
        p0 = _mm256_fmadd_ps(vb, _mm256_moveldup_ps(va), p0);
        q0 = _mm256_fmadd_ps(vb, _mm256_movehdup_ps(va), q0);
    }
    let p = _mm256_add_ps(p0, p1);
    let q = _mm256_permute_ps::<0xB1>(_mm256_add_ps(q0, q1));
    let combined = _mm256_fmsubadd_ps(p, _mm256_set1_ps(1.0), q);
    hsum_c32(combined) + scalar::conj_dot(a_tail, b_tail)
}

pub(super) fn dot(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f32> {
    if has_avx2_fma() {
        return unsafe { dot_avx2(left, right) };
    }
    scalar::dot(left, right)
}

#[target_feature(enable = "avx2,fma")]
unsafe fn dot_avx2(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f32> {
    let (a_blocks, a_rest) = left.as_chunks::<8>();
    let (b_blocks, b_rest) = right.as_chunks::<8>();
    let mut p0 = _mm256_setzero_ps();
    let mut q0 = _mm256_setzero_ps();
    let mut p1 = _mm256_setzero_ps();
    let mut q1 = _mm256_setzero_ps();
    for (a, b) in a_blocks.iter().zip(b_blocks) {
        let ap = a.as_ptr().cast::<f32>();
        let bp = b.as_ptr().cast::<f32>();
        let (va0, vb0) = unsafe { (_mm256_loadu_ps(ap), _mm256_loadu_ps(bp)) };
        let (va1, vb1) = unsafe { (_mm256_loadu_ps(ap.add(8)), _mm256_loadu_ps(bp.add(8))) };
        p0 = _mm256_fmadd_ps(va0, _mm256_moveldup_ps(vb0), p0);
        q0 = _mm256_fmadd_ps(va0, _mm256_movehdup_ps(vb0), q0);
        p1 = _mm256_fmadd_ps(va1, _mm256_moveldup_ps(vb1), p1);
        q1 = _mm256_fmadd_ps(va1, _mm256_movehdup_ps(vb1), q1);
    }
    let (a4, a_tail) = a_rest.as_chunks::<4>();
    let (b4, b_tail) = b_rest.as_chunks::<4>();
    for (a, b) in a4.iter().zip(b4) {
        let (va, vb) = unsafe {
            (
                _mm256_loadu_ps(a.as_ptr().cast()),
                _mm256_loadu_ps(b.as_ptr().cast()),
            )
        };
        p0 = _mm256_fmadd_ps(va, _mm256_moveldup_ps(vb), p0);
        q0 = _mm256_fmadd_ps(va, _mm256_movehdup_ps(vb), q0);
    }
    let p = _mm256_add_ps(p0, p1);
    let q = _mm256_permute_ps::<0xB1>(_mm256_add_ps(q0, q1));
    let combined = _mm256_fmaddsub_ps(p, _mm256_set1_ps(1.0), q);
    hsum_c32(combined) + scalar::dot(a_tail, b_tail)
}

pub(super) fn dot_f64(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f64> {
    if has_avx2_fma() {
        return unsafe { dot_f64_avx2(left, right) };
    }
    scalar::dot_f64(left, right)
}

#[target_feature(enable = "avx2,fma")]
unsafe fn dot_f64_avx2(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f64> {
    let (a_blocks, a_tail) = left.as_chunks::<4>();
    let (b_blocks, b_tail) = right.as_chunks::<4>();
    let mut p0 = _mm256_setzero_pd();
    let mut q0 = _mm256_setzero_pd();
    let mut p1 = _mm256_setzero_pd();
    let mut q1 = _mm256_setzero_pd();
    for (a, b) in a_blocks.iter().zip(b_blocks) {
        let ap = a.as_ptr().cast::<f32>();
        let bp = b.as_ptr().cast::<f32>();
        let va0 = _mm256_cvtps_pd(unsafe { _mm_loadu_ps(ap) });
        let vb0 = _mm256_cvtps_pd(unsafe { _mm_loadu_ps(bp) });
        let va1 = _mm256_cvtps_pd(unsafe { _mm_loadu_ps(ap.add(4)) });
        let vb1 = _mm256_cvtps_pd(unsafe { _mm_loadu_ps(bp.add(4)) });
        p0 = _mm256_fmadd_pd(va0, _mm256_unpacklo_pd(vb0, vb0), p0);
        q0 = _mm256_fmadd_pd(va0, _mm256_unpackhi_pd(vb0, vb0), q0);
        p1 = _mm256_fmadd_pd(va1, _mm256_unpacklo_pd(vb1, vb1), p1);
        q1 = _mm256_fmadd_pd(va1, _mm256_unpackhi_pd(vb1, vb1), q1);
    }
    let p = _mm256_add_pd(p0, p1);
    let q = _mm256_permute_pd::<0b0101>(_mm256_add_pd(q0, q1));
    let combined = _mm256_fmaddsub_pd(p, _mm256_set1_pd(1.0), q);
    hsum_c64(combined) + scalar::dot_f64(a_tail, b_tail)
}

pub(super) fn decay_cross_add(
    acc: &mut [Complex<f32>],
    decay: f32,
    left: &[Complex<f32>],
    right: &[Complex<f32>],
) {
    if has_avx2_fma() {
        unsafe { decay_cross_add_avx2(acc, decay, left, right) };
        return;
    }
    scalar::decay_cross_add(acc, decay, left, right);
}

#[target_feature(enable = "avx2,fma")]
unsafe fn decay_cross_add_avx2(
    acc: &mut [Complex<f32>],
    decay: f32,
    left: &[Complex<f32>],
    right: &[Complex<f32>],
) {
    let (acc_blocks, acc_tail) = acc.as_chunks_mut::<4>();
    let (a_blocks, a_tail) = left.as_chunks::<4>();
    let (b_blocks, b_tail) = right.as_chunks::<4>();
    let vd = _mm256_set1_ps(decay);
    for ((slot, a), b) in acc_blocks.iter_mut().zip(a_blocks).zip(b_blocks) {
        let sp = slot.as_mut_ptr().cast::<f32>();
        let vs = unsafe { _mm256_loadu_ps(sp) };
        let va = unsafe { _mm256_loadu_ps(a.as_ptr().cast()) };
        let vb = unsafe { _mm256_loadu_ps(b.as_ptr().cast()) };
        let prod = _mm256_fmsubadd_ps(
            va,
            _mm256_moveldup_ps(vb),
            _mm256_mul_ps(_mm256_permute_ps::<0xB1>(va), _mm256_movehdup_ps(vb)),
        );
        unsafe { _mm256_storeu_ps(sp, _mm256_fmadd_ps(vs, vd, prod)) };
    }
    scalar::decay_cross_add(acc_tail, decay, a_tail, b_tail);
}

pub(super) fn add_cross_f64(
    acc: &mut [Complex<f64>],
    left: &[Complex<f32>],
    right: &[Complex<f32>],
) {
    if has_avx2_fma() {
        unsafe { add_cross_f64_avx2(acc, left, right) };
        return;
    }
    scalar::add_cross_f64(acc, left, right);
}

#[target_feature(enable = "avx2,fma")]
unsafe fn add_cross_f64_avx2(
    acc: &mut [Complex<f64>],
    left: &[Complex<f32>],
    right: &[Complex<f32>],
) {
    let (acc_blocks, acc_tail) = acc.as_chunks_mut::<4>();
    let (a_blocks, a_tail) = left.as_chunks::<4>();
    let (b_blocks, b_tail) = right.as_chunks::<4>();
    for ((slot, a), b) in acc_blocks.iter_mut().zip(a_blocks).zip(b_blocks) {
        let sp = slot.as_mut_ptr().cast::<f64>();
        let ap = a.as_ptr().cast::<f32>();
        let bp = b.as_ptr().cast::<f32>();
        let va0 = _mm256_cvtps_pd(unsafe { _mm_loadu_ps(ap) });
        let vb0 = _mm256_cvtps_pd(unsafe { _mm_loadu_ps(bp) });
        let prod0 = _mm256_fmsubadd_pd(
            va0,
            _mm256_unpacklo_pd(vb0, vb0),
            _mm256_mul_pd(
                _mm256_permute_pd::<0b0101>(va0),
                _mm256_unpackhi_pd(vb0, vb0),
            ),
        );
        unsafe { _mm256_storeu_pd(sp, _mm256_add_pd(_mm256_loadu_pd(sp), prod0)) };

        let va1 = _mm256_cvtps_pd(unsafe { _mm_loadu_ps(ap.add(4)) });
        let vb1 = _mm256_cvtps_pd(unsafe { _mm_loadu_ps(bp.add(4)) });
        let prod1 = _mm256_fmsubadd_pd(
            va1,
            _mm256_unpacklo_pd(vb1, vb1),
            _mm256_mul_pd(
                _mm256_permute_pd::<0b0101>(va1),
                _mm256_unpackhi_pd(vb1, vb1),
            ),
        );
        unsafe { _mm256_storeu_pd(sp.add(4), _mm256_add_pd(_mm256_loadu_pd(sp.add(4)), prod1)) };
    }
    scalar::add_cross_f64(acc_tail, a_tail, b_tail);
}

pub(super) fn add_norm_sqr_f64(acc: &mut [f64], source: &[Complex<f32>]) {
    if has_avx2_fma() {
        unsafe { add_norm_sqr_f64_avx2(acc, source) };
        return;
    }
    scalar::add_norm_sqr_f64(acc, source);
}

#[target_feature(enable = "avx2,fma")]
unsafe fn add_norm_sqr_f64_avx2(acc: &mut [f64], source: &[Complex<f32>]) {
    let (acc_blocks, acc_tail) = acc.as_chunks_mut::<4>();
    let (src_blocks, src_tail) = source.as_chunks::<4>();
    for (slot, src) in acc_blocks.iter_mut().zip(src_blocks) {
        let sp = src.as_ptr().cast::<f32>();
        let d0 = _mm256_cvtps_pd(unsafe { _mm_loadu_ps(sp) });
        let d1 = _mm256_cvtps_pd(unsafe { _mm_loadu_ps(sp.add(4)) });
        let sq0 = _mm256_mul_pd(d0, d0);
        let sq1 = _mm256_mul_pd(d1, d1);
        let h = std::arch::x86_64::_mm256_hadd_pd(sq0, sq1);
        let ordered = _mm256_permute4x64_pd::<0xD8>(h);
        let dp = slot.as_mut_ptr();
        unsafe { _mm256_storeu_pd(dp, _mm256_add_pd(_mm256_loadu_pd(dp), ordered)) };
    }
    scalar::add_norm_sqr_f64(acc_tail, src_tail);
}

pub(super) fn add_rotated_f64(
    acc: &mut [Complex<f64>],
    source: &[Complex<f32>],
    turn: Complex<f64>,
) {
    if has_avx2_fma() {
        unsafe { add_rotated_f64_avx2(acc, source, turn) };
        return;
    }
    scalar::add_rotated_f64(acc, source, turn);
}

#[target_feature(enable = "avx2,fma")]
unsafe fn add_rotated_f64_avx2(
    acc: &mut [Complex<f64>],
    source: &[Complex<f32>],
    turn: Complex<f64>,
) {
    let (acc_blocks, acc_tail) = acc.as_chunks_mut::<4>();
    let (src_blocks, src_tail) = source.as_chunks::<4>();
    if turn == Complex::new(1.0, 0.0) {
        for (slot, src) in acc_blocks.iter_mut().zip(src_blocks) {
            let sp = slot.as_mut_ptr().cast::<f64>();
            let xp = src.as_ptr().cast::<f32>();
            let v0 = _mm256_cvtps_pd(unsafe { _mm_loadu_ps(xp) });
            let v1 = _mm256_cvtps_pd(unsafe { _mm_loadu_ps(xp.add(4)) });
            unsafe {
                _mm256_storeu_pd(sp, _mm256_add_pd(_mm256_loadu_pd(sp), v0));
                _mm256_storeu_pd(sp.add(4), _mm256_add_pd(_mm256_loadu_pd(sp.add(4)), v1));
            }
        }
    } else {
        let tr = _mm256_set1_pd(turn.re);
        let ti = _mm256_set1_pd(turn.im);
        for (slot, src) in acc_blocks.iter_mut().zip(src_blocks) {
            let sp = slot.as_mut_ptr().cast::<f64>();
            let xp = src.as_ptr().cast::<f32>();
            let v0 = _mm256_cvtps_pd(unsafe { _mm_loadu_ps(xp) });
            let p0 = _mm256_fmaddsub_pd(v0, tr, _mm256_mul_pd(_mm256_permute_pd::<0b0101>(v0), ti));
            unsafe { _mm256_storeu_pd(sp, _mm256_add_pd(_mm256_loadu_pd(sp), p0)) };

            let v1 = _mm256_cvtps_pd(unsafe { _mm_loadu_ps(xp.add(4)) });
            let p1 = _mm256_fmaddsub_pd(v1, tr, _mm256_mul_pd(_mm256_permute_pd::<0b0101>(v1), ti));
            unsafe { _mm256_storeu_pd(sp.add(4), _mm256_add_pd(_mm256_loadu_pd(sp.add(4)), p1)) };
        }
    }
    scalar::add_rotated_f64(acc_tail, src_tail, turn);
}

pub(super) fn axpy_norm_sqr(
    target: &mut [Complex<f32>],
    source: &[Complex<f32>],
    gain: Complex<f32>,
) -> f32 {
    if has_avx2_fma() {
        return unsafe { axpy_norm_sqr_avx2(target, source, gain) };
    }
    scalar::axpy_norm_sqr(target, source, gain)
}

#[target_feature(enable = "avx2,fma")]
unsafe fn axpy_norm_sqr_avx2(
    target: &mut [Complex<f32>],
    source: &[Complex<f32>],
    gain: Complex<f32>,
) -> f32 {
    let (h_blocks, h_tail) = target.as_chunks_mut::<4>();
    let (u_blocks, u_tail) = source.as_chunks::<4>();
    let gr = _mm256_set1_ps(gain.re);
    let gi = _mm256_set1_ps(gain.im);
    let mut norm = _mm256_setzero_ps();
    for (h, u) in h_blocks.iter_mut().zip(u_blocks) {
        let hp = h.as_mut_ptr().cast::<f32>();
        let vh = unsafe { _mm256_loadu_ps(hp) };
        let vu = unsafe { _mm256_loadu_ps(u.as_ptr().cast()) };
        let prod = _mm256_fmaddsub_ps(vu, gr, _mm256_mul_ps(_mm256_permute_ps::<0xB1>(vu), gi));
        let next = _mm256_add_ps(vh, prod);
        unsafe { _mm256_storeu_ps(hp, next) };
        norm = _mm256_fmadd_ps(next, next, norm);
    }
    hsum_f32(norm) + scalar::axpy_norm_sqr(h_tail, u_tail, gain)
}

pub(super) fn axpy_in_place(
    target: &mut [Complex<f32>],
    source: &[Complex<f32>],
    gain: Complex<f32>,
) {
    if has_avx2_fma() {
        unsafe { axpy_in_place_avx2(target, source, gain) };
        return;
    }
    scalar::axpy_in_place(target, source, gain);
}

#[target_feature(enable = "avx2,fma")]
unsafe fn axpy_in_place_avx2(
    target: &mut [Complex<f32>],
    source: &[Complex<f32>],
    gain: Complex<f32>,
) {
    let (h_blocks, h_tail) = target.as_chunks_mut::<4>();
    let (u_blocks, u_tail) = source.as_chunks::<4>();
    let gr = _mm256_set1_ps(gain.re);
    let gi = _mm256_set1_ps(gain.im);
    for (h, u) in h_blocks.iter_mut().zip(u_blocks) {
        let hp = h.as_mut_ptr().cast::<f32>();
        let vh = unsafe { _mm256_loadu_ps(hp) };
        let vu = unsafe { _mm256_loadu_ps(u.as_ptr().cast()) };
        let prod = _mm256_addsub_ps(
            _mm256_mul_ps(vu, gr),
            _mm256_mul_ps(_mm256_permute_ps::<0xB1>(vu), gi),
        );
        unsafe { _mm256_storeu_ps(hp, _mm256_add_ps(vh, prod)) };
    }
    scalar::axpy_in_place(h_tail, u_tail, gain);
}

pub(super) fn mul_conj_in_place(target: &mut [Complex<f32>], reference: &[Complex<f32>]) {
    if has_avx2_fma() {
        unsafe { mul_conj_in_place_avx2(target, reference) };
        return;
    }
    scalar::mul_conj_in_place(target, reference);
}

#[target_feature(enable = "avx2,fma")]
unsafe fn mul_conj_in_place_avx2(target: &mut [Complex<f32>], reference: &[Complex<f32>]) {
    let (dst_blocks, dst_tail) = target.as_chunks_mut::<4>();
    let (ref_blocks, ref_tail) = reference.as_chunks::<4>();
    for (dst, src) in dst_blocks.iter_mut().zip(ref_blocks) {
        let dp = dst.as_mut_ptr().cast::<f32>();
        let vd = unsafe { _mm256_loadu_ps(dp) };
        let vs = unsafe { _mm256_loadu_ps(src.as_ptr().cast()) };
        let res = _mm256_fmsubadd_ps(
            vd,
            _mm256_moveldup_ps(vs),
            _mm256_mul_ps(_mm256_permute_ps::<0xB1>(vd), _mm256_movehdup_ps(vs)),
        );
        unsafe { _mm256_storeu_ps(dp, res) };
    }
    scalar::mul_conj_in_place(dst_tail, ref_tail);
}

pub(super) fn mul_in_place(target: &mut [Complex<f32>], reference: &[Complex<f32>]) {
    if has_avx2_fma() {
        unsafe { mul_in_place_avx2(target, reference) };
        return;
    }
    scalar::mul_in_place(target, reference);
}

#[target_feature(enable = "avx2,fma")]
unsafe fn mul_in_place_avx2(target: &mut [Complex<f32>], reference: &[Complex<f32>]) {
    let (dst_blocks, dst_tail) = target.as_chunks_mut::<4>();
    let (ref_blocks, ref_tail) = reference.as_chunks::<4>();
    for (dst, src) in dst_blocks.iter_mut().zip(ref_blocks) {
        let dp = dst.as_mut_ptr().cast::<f32>();
        let vd = unsafe { _mm256_loadu_ps(dp) };
        let vs = unsafe { _mm256_loadu_ps(src.as_ptr().cast()) };
        let res = _mm256_fmaddsub_ps(
            vd,
            _mm256_moveldup_ps(vs),
            _mm256_mul_ps(_mm256_permute_ps::<0xB1>(vd), _mm256_movehdup_ps(vs)),
        );
        unsafe { _mm256_storeu_ps(dp, res) };
    }
    scalar::mul_in_place(dst_tail, ref_tail);
}

pub(super) fn sub_mul_into(
    target: &mut [Complex<f32>],
    base: &[Complex<f32>],
    left: &[Complex<f32>],
    right: &[Complex<f32>],
) {
    if has_avx2_fma() {
        unsafe { sub_mul_into_avx2(target, base, left, right) };
        return;
    }
    scalar::sub_mul_into(target, base, left, right);
}

#[target_feature(enable = "avx2,fma")]
unsafe fn sub_mul_into_avx2(
    target: &mut [Complex<f32>],
    base: &[Complex<f32>],
    left: &[Complex<f32>],
    right: &[Complex<f32>],
) {
    let (dst_blocks, dst_tail) = target.as_chunks_mut::<4>();
    let (q_blocks, q_tail) = base.as_chunks::<4>();
    let (a_blocks, a_tail) = left.as_chunks::<4>();
    let (b_blocks, b_tail) = right.as_chunks::<4>();
    for (((dst, q), a), b) in dst_blocks
        .iter_mut()
        .zip(q_blocks)
        .zip(a_blocks)
        .zip(b_blocks)
    {
        let vq = unsafe { _mm256_loadu_ps(q.as_ptr().cast()) };
        let va = unsafe { _mm256_loadu_ps(a.as_ptr().cast()) };
        let vb = unsafe { _mm256_loadu_ps(b.as_ptr().cast()) };
        let prod = _mm256_fmaddsub_ps(
            va,
            _mm256_moveldup_ps(vb),
            _mm256_mul_ps(_mm256_permute_ps::<0xB1>(va), _mm256_movehdup_ps(vb)),
        );
        unsafe { _mm256_storeu_ps(dst.as_mut_ptr().cast(), _mm256_sub_ps(vq, prod)) };
    }
    scalar::sub_mul_into(dst_tail, q_tail, a_tail, b_tail);
}

pub(super) fn sum_norm_sqr(samples: &[Complex<f32>]) -> f32 {
    if has_avx2_fma() {
        return unsafe { sum_norm_sqr_avx2(samples) };
    }
    scalar::sum_norm_sqr(samples)
}

#[target_feature(enable = "avx2,fma")]
unsafe fn sum_norm_sqr_avx2(samples: &[Complex<f32>]) -> f32 {
    let floats =
        unsafe { std::slice::from_raw_parts(samples.as_ptr().cast::<f32>(), 2 * samples.len()) };
    let (blocks, rest) = floats.as_chunks::<32>();
    let mut s0 = _mm256_setzero_ps();
    let mut s1 = _mm256_setzero_ps();
    let mut s2 = _mm256_setzero_ps();
    let mut s3 = _mm256_setzero_ps();
    for block in blocks {
        let p = block.as_ptr();
        let (v0, v1, v2, v3) = unsafe {
            (
                _mm256_loadu_ps(p),
                _mm256_loadu_ps(p.add(8)),
                _mm256_loadu_ps(p.add(16)),
                _mm256_loadu_ps(p.add(24)),
            )
        };
        s0 = _mm256_fmadd_ps(v0, v0, s0);
        s1 = _mm256_fmadd_ps(v1, v1, s1);
        s2 = _mm256_fmadd_ps(v2, v2, s2);
        s3 = _mm256_fmadd_ps(v3, v3, s3);
    }
    let (eights, tail) = rest.as_chunks::<8>();
    for eight in eights {
        let v = unsafe { _mm256_loadu_ps(eight.as_ptr()) };
        s0 = _mm256_fmadd_ps(v, v, s0);
    }
    let tail_sum: f32 = tail.iter().map(|&x| x * x).sum();
    hsum_f32(_mm256_add_ps(_mm256_add_ps(s0, s1), _mm256_add_ps(s2, s3))) + tail_sum
}

pub(super) fn scale_real_into(
    target: &mut [Complex<f32>],
    source: &[Complex<f32>],
    weights: &[f32],
) {
    if has_avx2_fma() {
        unsafe { scale_real_into_avx2(target, source, weights) };
        return;
    }
    scalar::scale_real_into(target, source, weights);
}

#[target_feature(enable = "avx2,fma")]
unsafe fn scale_real_into_avx2(
    target: &mut [Complex<f32>],
    source: &[Complex<f32>],
    weights: &[f32],
) {
    let (dst_blocks, dst_tail) = target.as_chunks_mut::<4>();
    let (src_blocks, src_tail) = source.as_chunks::<4>();
    let (w_blocks, w_tail) = weights.as_chunks::<4>();
    for ((dst, src), w) in dst_blocks.iter_mut().zip(src_blocks).zip(w_blocks) {
        let vs = unsafe { _mm256_loadu_ps(src.as_ptr().cast()) };
        let vw = dup_quad_f32(unsafe { _mm_loadu_ps(w.as_ptr()) });
        unsafe { _mm256_storeu_ps(dst.as_mut_ptr().cast(), _mm256_mul_ps(vs, vw)) };
    }
    scalar::scale_real_into(dst_tail, src_tail, w_tail);
}

pub(super) fn scale_real_in_place(target: &mut [Complex<f32>], weights: &[f32]) {
    if has_avx2_fma() {
        unsafe { scale_real_in_place_avx2(target, weights) };
        return;
    }
    scalar::scale_real_in_place(target, weights);
}

#[target_feature(enable = "avx2,fma")]
unsafe fn scale_real_in_place_avx2(target: &mut [Complex<f32>], weights: &[f32]) {
    let (dst_blocks, dst_tail) = target.as_chunks_mut::<4>();
    let (w_blocks, w_tail) = weights.as_chunks::<4>();
    for (dst, w) in dst_blocks.iter_mut().zip(w_blocks) {
        let dp = dst.as_mut_ptr().cast::<f32>();
        let vd = unsafe { _mm256_loadu_ps(dp) };
        let vw = dup_quad_f32(unsafe { _mm_loadu_ps(w.as_ptr()) });
        unsafe { _mm256_storeu_ps(dp, _mm256_mul_ps(vd, vw)) };
    }
    scalar::scale_real_in_place(dst_tail, w_tail);
}

pub(super) unsafe fn magnitudes_raw(iq: &[Complex<f32>], dst: *mut f32) {
    if has_avx2_fma() {
        unsafe { magnitudes_raw_avx2(iq, dst) };
        return;
    }
    unsafe { scalar::magnitudes_raw(iq, dst) };
}

#[target_feature(enable = "avx2,fma")]
unsafe fn magnitudes_raw_avx2(iq: &[Complex<f32>], dst: *mut f32) {
    let (blocks, tail) = iq.as_chunks::<8>();
    let mut offset = 0;
    for block in blocks {
        let p = block.as_ptr().cast::<f32>();
        let (v0, v1) = unsafe { (_mm256_loadu_ps(p), _mm256_loadu_ps(p.add(8))) };
        let sq0 = _mm256_mul_ps(v0, v0);
        let sq1 = _mm256_mul_ps(v1, v1);
        let sum = _mm256_hadd_ps(sq0, sq1);
        let ordered = _mm256_castpd_ps(_mm256_permute4x64_pd::<0xD8>(_mm256_castps_pd(sum)));
        unsafe { _mm256_storeu_ps(dst.add(offset), _mm256_sqrt_ps(ordered)) };
        offset += 8;
    }
    unsafe { scalar::magnitudes_raw(tail, dst.add(offset)) };
}

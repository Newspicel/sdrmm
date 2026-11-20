use num_complex::Complex;

use super::widen;

const FOLD_CHUNK: usize = 2048;

#[must_use]
pub fn cross_sum(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f64> {
    let len = left.len().min(right.len());
    let (left, right) = (&left[..len], &right[..len]);
    #[cfg(target_arch = "aarch64")]
    {
        neon::cross_sum(left, right)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        scalar::cross_sum(left, right)
    }
}

#[must_use]
pub fn conj_dot(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f32> {
    let len = left.len().min(right.len());
    let (left, right) = (&left[..len], &right[..len]);
    #[cfg(target_arch = "aarch64")]
    {
        neon::conj_dot(left, right)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        scalar::conj_dot(left, right)
    }
}

#[must_use]
pub fn dot(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f32> {
    let len = left.len().min(right.len());
    let (left, right) = (&left[..len], &right[..len]);
    #[cfg(target_arch = "aarch64")]
    {
        neon::dot(left, right)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        scalar::dot(left, right)
    }
}

#[must_use]
pub fn dot_f64(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f64> {
    let len = left.len().min(right.len());
    let (left, right) = (&left[..len], &right[..len]);
    #[cfg(target_arch = "aarch64")]
    {
        neon::dot_f64(left, right)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        scalar::dot_f64(left, right)
    }
}

pub fn decay_cross_add(
    acc: &mut [Complex<f32>],
    decay: f32,
    left: &[Complex<f32>],
    right: &[Complex<f32>],
) {
    let len = acc.len().min(left.len()).min(right.len());
    let (acc, left, right) = (&mut acc[..len], &left[..len], &right[..len]);
    #[cfg(target_arch = "aarch64")]
    {
        neon::decay_cross_add(acc, decay, left, right);
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        scalar::decay_cross_add(acc, decay, left, right);
    }
}

pub fn add_cross_f64(acc: &mut [Complex<f64>], left: &[Complex<f32>], right: &[Complex<f32>]) {
    let len = acc.len().min(left.len()).min(right.len());
    let (acc, left, right) = (&mut acc[..len], &left[..len], &right[..len]);
    #[cfg(target_arch = "aarch64")]
    {
        neon::add_cross_f64(acc, left, right);
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        scalar::add_cross_f64(acc, left, right);
    }
}

pub fn add_norm_sqr_f64(acc: &mut [f64], source: &[Complex<f32>]) {
    let len = acc.len().min(source.len());
    let (acc, source) = (&mut acc[..len], &source[..len]);
    #[cfg(target_arch = "aarch64")]
    {
        neon::add_norm_sqr_f64(acc, source);
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        scalar::add_norm_sqr_f64(acc, source);
    }
}

pub fn add_rotated_f64(acc: &mut [Complex<f64>], source: &[Complex<f32>], turn: Complex<f64>) {
    let len = acc.len().min(source.len());
    let (acc, source) = (&mut acc[..len], &source[..len]);
    #[cfg(target_arch = "aarch64")]
    {
        neon::add_rotated_f64(acc, source, turn);
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        scalar::add_rotated_f64(acc, source, turn);
    }
}

pub fn axpy_norm_sqr(
    target: &mut [Complex<f32>],
    source: &[Complex<f32>],
    gain: Complex<f32>,
) -> f32 {
    let len = target.len().min(source.len());
    let (target, source) = (&mut target[..len], &source[..len]);
    #[cfg(target_arch = "aarch64")]
    {
        neon::axpy_norm_sqr(target, source, gain)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        scalar::axpy_norm_sqr(target, source, gain)
    }
}

pub fn mul_conj_in_place(target: &mut [Complex<f32>], reference: &[Complex<f32>]) {
    let len = target.len().min(reference.len());
    let (target, reference) = (&mut target[..len], &reference[..len]);
    #[cfg(target_arch = "aarch64")]
    {
        neon::mul_conj_in_place(target, reference);
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        scalar::mul_conj_in_place(target, reference);
    }
}

#[must_use]
pub fn sum_norm_sqr(samples: &[Complex<f32>]) -> f32 {
    #[cfg(target_arch = "aarch64")]
    {
        neon::sum_norm_sqr(samples)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        scalar::sum_norm_sqr(samples)
    }
}

pub fn scale_real_into(target: &mut [Complex<f32>], source: &[Complex<f32>], weights: &[f32]) {
    let len = target.len().min(source.len()).min(weights.len());
    let (target, source, weights) = (&mut target[..len], &source[..len], &weights[..len]);
    #[cfg(target_arch = "aarch64")]
    {
        neon::scale_real_into(target, source, weights);
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        scalar::scale_real_into(target, source, weights);
    }
}

pub fn magnitudes_into(iq: &[Complex<f32>], out: &mut Vec<f32>) {
    let base = out.len();
    out.reserve(iq.len());
    let spare = &mut out.spare_capacity_mut()[..iq.len()];
    let dst = spare.as_mut_ptr().cast::<f32>();
    #[cfg(target_arch = "aarch64")]
    unsafe {
        neon::magnitudes_raw(iq, dst);
    }
    #[cfg(not(target_arch = "aarch64"))]
    unsafe {
        scalar::magnitudes_raw(iq, dst);
    }
    unsafe {
        out.set_len(base + iq.len());
    }
}

mod scalar {
    use num_complex::Complex;

    use super::{FOLD_CHUNK, widen};
    use crate::math::fastmath::madd;

    #[allow(dead_code)]
    pub(super) fn cross_sum(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f64> {
        let mut total = Complex::new(0.0f64, 0.0);
        for (a_chunk, b_chunk) in left.chunks(FOLD_CHUNK).zip(right.chunks(FOLD_CHUNK)) {
            let mut re = 0.0f32;
            let mut im = 0.0f32;
            for (a, b) in a_chunk.iter().zip(b_chunk) {
                re = madd(a.im, b.im, madd(a.re, b.re, re));
                im = madd(-a.re, b.im, madd(a.im, b.re, im));
            }
            total += Complex::new(f64::from(re), f64::from(im));
        }
        total
    }

    #[allow(dead_code)]
    pub(super) fn conj_dot(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f32> {
        let mut re = 0.0f32;
        let mut im = 0.0f32;
        for (a, b) in left.iter().zip(right) {
            re = madd(a.im, b.im, madd(a.re, b.re, re));
            im = madd(-a.im, b.re, madd(a.re, b.im, im));
        }
        Complex::new(re, im)
    }

    #[allow(dead_code)]
    pub(super) fn dot(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f32> {
        let mut re = 0.0f32;
        let mut im = 0.0f32;
        for (a, b) in left.iter().zip(right) {
            re = madd(-a.im, b.im, madd(a.re, b.re, re));
            im = madd(a.im, b.re, madd(a.re, b.im, im));
        }
        Complex::new(re, im)
    }

    #[allow(dead_code)]
    pub(super) fn dot_f64(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f64> {
        let mut re = 0.0f64;
        let mut im = 0.0f64;
        for (a, b) in left.iter().zip(right) {
            let p = a * b;
            re += f64::from(p.re);
            im += f64::from(p.im);
        }
        Complex::new(re, im)
    }

    #[allow(dead_code)]
    pub(super) fn decay_cross_add(
        acc: &mut [Complex<f32>],
        decay: f32,
        left: &[Complex<f32>],
        right: &[Complex<f32>],
    ) {
        for ((slot, a), b) in acc.iter_mut().zip(left).zip(right) {
            *slot = *slot * decay + a * b.conj();
        }
    }

    #[allow(dead_code)]
    pub(super) fn add_cross_f64(
        acc: &mut [Complex<f64>],
        left: &[Complex<f32>],
        right: &[Complex<f32>],
    ) {
        for ((slot, a), b) in acc.iter_mut().zip(left).zip(right) {
            *slot += widen(*a) * widen(*b).conj();
        }
    }

    #[allow(dead_code)]
    pub(super) fn add_norm_sqr_f64(acc: &mut [f64], source: &[Complex<f32>]) {
        for (slot, x) in acc.iter_mut().zip(source) {
            *slot += widen(*x).norm_sqr();
        }
    }

    #[allow(dead_code)]
    pub(super) fn add_rotated_f64(
        acc: &mut [Complex<f64>],
        source: &[Complex<f32>],
        turn: Complex<f64>,
    ) {
        if turn == Complex::new(1.0, 0.0) {
            for (slot, x) in acc.iter_mut().zip(source) {
                *slot += widen(*x);
            }
        } else {
            for (slot, x) in acc.iter_mut().zip(source) {
                *slot += turn * widen(*x);
            }
        }
    }

    #[allow(dead_code)]
    pub(super) fn axpy_norm_sqr(
        target: &mut [Complex<f32>],
        source: &[Complex<f32>],
        gain: Complex<f32>,
    ) -> f32 {
        let mut norm = 0.0f32;
        for (h, u) in target.iter_mut().zip(source) {
            *h += u * gain;
            norm += h.norm_sqr();
        }
        norm
    }

    #[allow(dead_code)]
    pub(super) fn mul_conj_in_place(target: &mut [Complex<f32>], reference: &[Complex<f32>]) {
        for (dst, src) in target.iter_mut().zip(reference) {
            *dst *= src.conj();
        }
    }

    #[allow(dead_code)]
    pub(super) fn sum_norm_sqr(samples: &[Complex<f32>]) -> f32 {
        samples.iter().map(Complex::norm_sqr).sum()
    }

    #[allow(dead_code)]
    pub(super) fn scale_real_into(
        target: &mut [Complex<f32>],
        source: &[Complex<f32>],
        weights: &[f32],
    ) {
        for ((dst, src), &w) in target.iter_mut().zip(source).zip(weights) {
            *dst = src * w;
        }
    }

    #[allow(dead_code)]
    pub(super) unsafe fn magnitudes_raw(iq: &[Complex<f32>], dst: *mut f32) {
        for (i, s) in iq.iter().enumerate() {
            unsafe {
                dst.add(i).write(madd(s.re, s.re, s.im * s.im).sqrt());
            }
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod neon {
    use std::arch::aarch64::{
        float32x4x2_t, float64x2x2_t, vaddq_f32, vaddq_f64, vaddvq_f32, vaddvq_f64, vcvt_f64_f32,
        vcvt_high_f64_f32, vdupq_n_f32, vdupq_n_f64, vfmaq_f32, vfmaq_f64, vfmsq_f32, vfmsq_f64,
        vget_low_f32, vld1q_f32, vld1q_f64, vld2q_f32, vld2q_f64, vmulq_f32, vsqrtq_f32, vst1q_f32,
        vst1q_f64, vst2q_f32, vst2q_f64,
    };

    use num_complex::Complex;

    use super::{FOLD_CHUNK, scalar};

    pub(super) fn cross_sum(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f64> {
        let mut total = Complex::new(0.0f64, 0.0);
        for (a_chunk, b_chunk) in left.chunks(FOLD_CHUNK).zip(right.chunks(FOLD_CHUNK)) {
            total += cross_sum_chunk(a_chunk, b_chunk);
        }
        total
    }

    fn cross_sum_chunk(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f64> {
        let (a_blocks, a_rest) = left.as_chunks::<8>();
        let (b_blocks, b_rest) = right.as_chunks::<8>();
        unsafe {
            let mut re0 = vdupq_n_f32(0.0);
            let mut im0 = vdupq_n_f32(0.0);
            let mut re1 = vdupq_n_f32(0.0);
            let mut im1 = vdupq_n_f32(0.0);
            for (a, b) in a_blocks.iter().zip(b_blocks) {
                let ap = a.as_ptr().cast::<f32>();
                let bp = b.as_ptr().cast::<f32>();
                let va0 = vld2q_f32(ap);
                let vb0 = vld2q_f32(bp);
                let va1 = vld2q_f32(ap.add(8));
                let vb1 = vld2q_f32(bp.add(8));
                re0 = vfmaq_f32(vfmaq_f32(re0, va0.0, vb0.0), va0.1, vb0.1);
                im0 = vfmsq_f32(vfmaq_f32(im0, va0.1, vb0.0), va0.0, vb0.1);
                re1 = vfmaq_f32(vfmaq_f32(re1, va1.0, vb1.0), va1.1, vb1.1);
                im1 = vfmsq_f32(vfmaq_f32(im1, va1.1, vb1.0), va1.0, vb1.1);
            }
            let (a4, a_tail) = a_rest.as_chunks::<4>();
            let (b4, b_tail) = b_rest.as_chunks::<4>();
            for (a, b) in a4.iter().zip(b4) {
                let va = vld2q_f32(a.as_ptr().cast());
                let vb = vld2q_f32(b.as_ptr().cast());
                re0 = vfmaq_f32(vfmaq_f32(re0, va.0, vb.0), va.1, vb.1);
                im0 = vfmsq_f32(vfmaq_f32(im0, va.1, vb.0), va.0, vb.1);
            }
            let sum_re = f64::from(vaddvq_f32(vaddq_f32(re0, re1)));
            let sum_im = f64::from(vaddvq_f32(vaddq_f32(im0, im1)));
            let tail = scalar::cross_sum(a_tail, b_tail);
            Complex::new(sum_re + tail.re, sum_im + tail.im)
        }
    }

    pub(super) fn conj_dot(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f32> {
        let (a_blocks, a_rest) = left.as_chunks::<8>();
        let (b_blocks, b_rest) = right.as_chunks::<8>();
        unsafe {
            let mut re0 = vdupq_n_f32(0.0);
            let mut im0 = vdupq_n_f32(0.0);
            let mut re1 = vdupq_n_f32(0.0);
            let mut im1 = vdupq_n_f32(0.0);
            for (a, b) in a_blocks.iter().zip(b_blocks) {
                let ap = a.as_ptr().cast::<f32>();
                let bp = b.as_ptr().cast::<f32>();
                let va0 = vld2q_f32(ap);
                let vb0 = vld2q_f32(bp);
                let va1 = vld2q_f32(ap.add(8));
                let vb1 = vld2q_f32(bp.add(8));
                re0 = vfmaq_f32(vfmaq_f32(re0, va0.0, vb0.0), va0.1, vb0.1);
                im0 = vfmsq_f32(vfmaq_f32(im0, va0.0, vb0.1), va0.1, vb0.0);
                re1 = vfmaq_f32(vfmaq_f32(re1, va1.0, vb1.0), va1.1, vb1.1);
                im1 = vfmsq_f32(vfmaq_f32(im1, va1.0, vb1.1), va1.1, vb1.0);
            }
            let (a4, a_tail) = a_rest.as_chunks::<4>();
            let (b4, b_tail) = b_rest.as_chunks::<4>();
            for (a, b) in a4.iter().zip(b4) {
                let va = vld2q_f32(a.as_ptr().cast());
                let vb = vld2q_f32(b.as_ptr().cast());
                re0 = vfmaq_f32(vfmaq_f32(re0, va.0, vb.0), va.1, vb.1);
                im0 = vfmsq_f32(vfmaq_f32(im0, va.0, vb.1), va.1, vb.0);
            }
            let tail = scalar::conj_dot(a_tail, b_tail);
            Complex::new(
                vaddvq_f32(vaddq_f32(re0, re1)) + tail.re,
                vaddvq_f32(vaddq_f32(im0, im1)) + tail.im,
            )
        }
    }

    pub(super) fn dot(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f32> {
        let (a_blocks, a_rest) = left.as_chunks::<8>();
        let (b_blocks, b_rest) = right.as_chunks::<8>();
        unsafe {
            let mut re0 = vdupq_n_f32(0.0);
            let mut im0 = vdupq_n_f32(0.0);
            let mut re1 = vdupq_n_f32(0.0);
            let mut im1 = vdupq_n_f32(0.0);
            for (a, b) in a_blocks.iter().zip(b_blocks) {
                let ap = a.as_ptr().cast::<f32>();
                let bp = b.as_ptr().cast::<f32>();
                let va0 = vld2q_f32(ap);
                let vb0 = vld2q_f32(bp);
                let va1 = vld2q_f32(ap.add(8));
                let vb1 = vld2q_f32(bp.add(8));
                re0 = vfmsq_f32(vfmaq_f32(re0, va0.0, vb0.0), va0.1, vb0.1);
                im0 = vfmaq_f32(vfmaq_f32(im0, va0.0, vb0.1), va0.1, vb0.0);
                re1 = vfmsq_f32(vfmaq_f32(re1, va1.0, vb1.0), va1.1, vb1.1);
                im1 = vfmaq_f32(vfmaq_f32(im1, va1.0, vb1.1), va1.1, vb1.0);
            }
            let (a4, a_tail) = a_rest.as_chunks::<4>();
            let (b4, b_tail) = b_rest.as_chunks::<4>();
            for (a, b) in a4.iter().zip(b4) {
                let va = vld2q_f32(a.as_ptr().cast());
                let vb = vld2q_f32(b.as_ptr().cast());
                re0 = vfmsq_f32(vfmaq_f32(re0, va.0, vb.0), va.1, vb.1);
                im0 = vfmaq_f32(vfmaq_f32(im0, va.0, vb.1), va.1, vb.0);
            }
            let tail = scalar::dot(a_tail, b_tail);
            Complex::new(
                vaddvq_f32(vaddq_f32(re0, re1)) + tail.re,
                vaddvq_f32(vaddq_f32(im0, im1)) + tail.im,
            )
        }
    }

    pub(super) fn dot_f64(left: &[Complex<f32>], right: &[Complex<f32>]) -> Complex<f64> {
        let (a_blocks, a_tail) = left.as_chunks::<4>();
        let (b_blocks, b_tail) = right.as_chunks::<4>();
        unsafe {
            let mut re0 = vdupq_n_f64(0.0);
            let mut im0 = vdupq_n_f64(0.0);
            let mut re1 = vdupq_n_f64(0.0);
            let mut im1 = vdupq_n_f64(0.0);
            for (a, b) in a_blocks.iter().zip(b_blocks) {
                let va = vld2q_f32(a.as_ptr().cast());
                let vb = vld2q_f32(b.as_ptr().cast());
                let ar0 = vcvt_f64_f32(vget_low_f32(va.0));
                let ai0 = vcvt_f64_f32(vget_low_f32(va.1));
                let br0 = vcvt_f64_f32(vget_low_f32(vb.0));
                let bi0 = vcvt_f64_f32(vget_low_f32(vb.1));
                re0 = vfmsq_f64(vfmaq_f64(re0, ar0, br0), ai0, bi0);
                im0 = vfmaq_f64(vfmaq_f64(im0, ar0, bi0), ai0, br0);

                let ar1 = vcvt_high_f64_f32(va.0);
                let ai1 = vcvt_high_f64_f32(va.1);
                let br1 = vcvt_high_f64_f32(vb.0);
                let bi1 = vcvt_high_f64_f32(vb.1);
                re1 = vfmsq_f64(vfmaq_f64(re1, ar1, br1), ai1, bi1);
                im1 = vfmaq_f64(vfmaq_f64(im1, ar1, bi1), ai1, br1);
            }
            let tail = scalar::dot_f64(a_tail, b_tail);
            Complex::new(
                vaddvq_f64(vaddq_f64(re0, re1)) + tail.re,
                vaddvq_f64(vaddq_f64(im0, im1)) + tail.im,
            )
        }
    }

    pub(super) fn decay_cross_add(
        acc: &mut [Complex<f32>],
        decay: f32,
        left: &[Complex<f32>],
        right: &[Complex<f32>],
    ) {
        let (acc_blocks, acc_tail) = acc.as_chunks_mut::<4>();
        let (a_blocks, a_tail) = left.as_chunks::<4>();
        let (b_blocks, b_tail) = right.as_chunks::<4>();
        unsafe {
            let vd = vdupq_n_f32(decay);
            for ((slot, a), b) in acc_blocks.iter_mut().zip(a_blocks).zip(b_blocks) {
                let sp = slot.as_mut_ptr().cast::<f32>();
                let vs = vld2q_f32(sp);
                let va = vld2q_f32(a.as_ptr().cast());
                let vb = vld2q_f32(b.as_ptr().cast());
                let re = vfmaq_f32(vfmaq_f32(vmulq_f32(vs.0, vd), va.0, vb.0), va.1, vb.1);
                let im = vfmsq_f32(vfmaq_f32(vmulq_f32(vs.1, vd), va.1, vb.0), va.0, vb.1);
                vst2q_f32(sp, float32x4x2_t(re, im));
            }
        }
        scalar::decay_cross_add(acc_tail, decay, a_tail, b_tail);
    }

    pub(super) fn add_cross_f64(
        acc: &mut [Complex<f64>],
        left: &[Complex<f32>],
        right: &[Complex<f32>],
    ) {
        let (acc_blocks, acc_tail) = acc.as_chunks_mut::<4>();
        let (a_blocks, a_tail) = left.as_chunks::<4>();
        let (b_blocks, b_tail) = right.as_chunks::<4>();
        unsafe {
            for ((slot, a), b) in acc_blocks.iter_mut().zip(a_blocks).zip(b_blocks) {
                let va = vld2q_f32(a.as_ptr().cast());
                let vb = vld2q_f32(b.as_ptr().cast());
                let sp = slot.as_mut_ptr().cast::<f64>();

                let ar0 = vcvt_f64_f32(vget_low_f32(va.0));
                let ai0 = vcvt_f64_f32(vget_low_f32(va.1));
                let br0 = vcvt_f64_f32(vget_low_f32(vb.0));
                let bi0 = vcvt_f64_f32(vget_low_f32(vb.1));
                let vs0 = vld2q_f64(sp);
                let re0 = vfmaq_f64(vfmaq_f64(vs0.0, ar0, br0), ai0, bi0);
                let im0 = vfmsq_f64(vfmaq_f64(vs0.1, ai0, br0), ar0, bi0);
                vst2q_f64(sp, float64x2x2_t(re0, im0));

                let ar1 = vcvt_high_f64_f32(va.0);
                let ai1 = vcvt_high_f64_f32(va.1);
                let br1 = vcvt_high_f64_f32(vb.0);
                let bi1 = vcvt_high_f64_f32(vb.1);
                let vs1 = vld2q_f64(sp.add(4));
                let re1 = vfmaq_f64(vfmaq_f64(vs1.0, ar1, br1), ai1, bi1);
                let im1 = vfmsq_f64(vfmaq_f64(vs1.1, ai1, br1), ar1, bi1);
                vst2q_f64(sp.add(4), float64x2x2_t(re1, im1));
            }
        }
        scalar::add_cross_f64(acc_tail, a_tail, b_tail);
    }

    pub(super) fn add_norm_sqr_f64(acc: &mut [f64], source: &[Complex<f32>]) {
        let (acc_blocks, acc_tail) = acc.as_chunks_mut::<4>();
        let (src_blocks, src_tail) = source.as_chunks::<4>();
        unsafe {
            for (slot, src) in acc_blocks.iter_mut().zip(src_blocks) {
                let vs = vld2q_f32(src.as_ptr().cast());
                let sp = slot.as_mut_ptr();
                let r0 = vcvt_f64_f32(vget_low_f32(vs.0));
                let i0 = vcvt_f64_f32(vget_low_f32(vs.1));
                let r1 = vcvt_high_f64_f32(vs.0);
                let i1 = vcvt_high_f64_f32(vs.1);
                let out0 = vfmaq_f64(vfmaq_f64(vld1q_f64(sp), r0, r0), i0, i0);
                let out1 = vfmaq_f64(vfmaq_f64(vld1q_f64(sp.add(2)), r1, r1), i1, i1);
                vst1q_f64(sp, out0);
                vst1q_f64(sp.add(2), out1);
            }
        }
        scalar::add_norm_sqr_f64(acc_tail, src_tail);
    }

    pub(super) fn add_rotated_f64(
        acc: &mut [Complex<f64>],
        source: &[Complex<f32>],
        turn: Complex<f64>,
    ) {
        let (acc_blocks, acc_tail) = acc.as_chunks_mut::<4>();
        let (src_blocks, src_tail) = source.as_chunks::<4>();
        unsafe {
            if turn == Complex::new(1.0, 0.0) {
                for (slot, src) in acc_blocks.iter_mut().zip(src_blocks) {
                    let vs = vld2q_f32(src.as_ptr().cast());
                    let sp = slot.as_mut_ptr().cast::<f64>();
                    let r0 = vcvt_f64_f32(vget_low_f32(vs.0));
                    let i0 = vcvt_f64_f32(vget_low_f32(vs.1));
                    let r1 = vcvt_high_f64_f32(vs.0);
                    let i1 = vcvt_high_f64_f32(vs.1);
                    let va0 = vld2q_f64(sp);
                    let va1 = vld2q_f64(sp.add(4));
                    vst2q_f64(
                        sp,
                        float64x2x2_t(vaddq_f64(va0.0, r0), vaddq_f64(va0.1, i0)),
                    );
                    vst2q_f64(
                        sp.add(4),
                        float64x2x2_t(vaddq_f64(va1.0, r1), vaddq_f64(va1.1, i1)),
                    );
                }
            } else {
                let tr = vdupq_n_f64(turn.re);
                let ti = vdupq_n_f64(turn.im);
                for (slot, src) in acc_blocks.iter_mut().zip(src_blocks) {
                    let vs = vld2q_f32(src.as_ptr().cast());
                    let sp = slot.as_mut_ptr().cast::<f64>();

                    let r0 = vcvt_f64_f32(vget_low_f32(vs.0));
                    let i0 = vcvt_f64_f32(vget_low_f32(vs.1));
                    let va0 = vld2q_f64(sp);
                    let re0 = vfmsq_f64(vfmaq_f64(va0.0, tr, r0), ti, i0);
                    let im0 = vfmaq_f64(vfmaq_f64(va0.1, tr, i0), ti, r0);
                    vst2q_f64(sp, float64x2x2_t(re0, im0));

                    let r1 = vcvt_high_f64_f32(vs.0);
                    let i1 = vcvt_high_f64_f32(vs.1);
                    let va1 = vld2q_f64(sp.add(4));
                    let re1 = vfmsq_f64(vfmaq_f64(va1.0, tr, r1), ti, i1);
                    let im1 = vfmaq_f64(vfmaq_f64(va1.1, tr, i1), ti, r1);
                    vst2q_f64(sp.add(4), float64x2x2_t(re1, im1));
                }
            }
        }
        scalar::add_rotated_f64(acc_tail, src_tail, turn);
    }

    pub(super) fn axpy_norm_sqr(
        target: &mut [Complex<f32>],
        source: &[Complex<f32>],
        gain: Complex<f32>,
    ) -> f32 {
        let (h_blocks, h_tail) = target.as_chunks_mut::<4>();
        let (u_blocks, u_tail) = source.as_chunks::<4>();
        let vec_norm = unsafe {
            let gr = vdupq_n_f32(gain.re);
            let gi = vdupq_n_f32(gain.im);
            let mut norm = vdupq_n_f32(0.0);
            for (h, u) in h_blocks.iter_mut().zip(u_blocks) {
                let hp = h.as_mut_ptr().cast::<f32>();
                let vh = vld2q_f32(hp);
                let vu = vld2q_f32(u.as_ptr().cast());
                let re = vfmsq_f32(vfmaq_f32(vh.0, vu.0, gr), vu.1, gi);
                let im = vfmaq_f32(vfmaq_f32(vh.1, vu.0, gi), vu.1, gr);
                vst2q_f32(hp, float32x4x2_t(re, im));
                norm = vfmaq_f32(vfmaq_f32(norm, re, re), im, im);
            }
            vaddvq_f32(norm)
        };
        vec_norm + scalar::axpy_norm_sqr(h_tail, u_tail, gain)
    }

    pub(super) fn mul_conj_in_place(target: &mut [Complex<f32>], reference: &[Complex<f32>]) {
        let (dst_blocks, dst_tail) = target.as_chunks_mut::<4>();
        let (ref_blocks, ref_tail) = reference.as_chunks::<4>();
        unsafe {
            for (dst, src) in dst_blocks.iter_mut().zip(ref_blocks) {
                let dp = dst.as_mut_ptr().cast::<f32>();
                let vd = vld2q_f32(dp);
                let vs = vld2q_f32(src.as_ptr().cast());
                let re = vfmaq_f32(vmulq_f32(vd.0, vs.0), vd.1, vs.1);
                let im = vfmsq_f32(vmulq_f32(vd.1, vs.0), vd.0, vs.1);
                vst2q_f32(dp, float32x4x2_t(re, im));
            }
        }
        scalar::mul_conj_in_place(dst_tail, ref_tail);
    }

    pub(super) fn sum_norm_sqr(samples: &[Complex<f32>]) -> f32 {
        let floats = unsafe {
            std::slice::from_raw_parts(samples.as_ptr().cast::<f32>(), 2 * samples.len())
        };
        let (blocks, rest) = floats.as_chunks::<16>();
        unsafe {
            let mut s0 = vdupq_n_f32(0.0);
            let mut s1 = vdupq_n_f32(0.0);
            let mut s2 = vdupq_n_f32(0.0);
            let mut s3 = vdupq_n_f32(0.0);
            for block in blocks {
                let p = block.as_ptr();
                let v0 = vld1q_f32(p);
                let v1 = vld1q_f32(p.add(4));
                let v2 = vld1q_f32(p.add(8));
                let v3 = vld1q_f32(p.add(12));
                s0 = vfmaq_f32(s0, v0, v0);
                s1 = vfmaq_f32(s1, v1, v1);
                s2 = vfmaq_f32(s2, v2, v2);
                s3 = vfmaq_f32(s3, v3, v3);
            }
            let (quads, tail) = rest.as_chunks::<4>();
            for quad in quads {
                let v = vld1q_f32(quad.as_ptr());
                s0 = vfmaq_f32(s0, v, v);
            }
            let tail_sum: f32 = tail.iter().map(|&x| x * x).sum();
            vaddvq_f32(vaddq_f32(vaddq_f32(s0, s1), vaddq_f32(s2, s3))) + tail_sum
        }
    }

    pub(super) fn scale_real_into(
        target: &mut [Complex<f32>],
        source: &[Complex<f32>],
        weights: &[f32],
    ) {
        let (dst_blocks, dst_tail) = target.as_chunks_mut::<4>();
        let (src_blocks, src_tail) = source.as_chunks::<4>();
        let (w_blocks, w_tail) = weights.as_chunks::<4>();
        unsafe {
            for ((dst, src), w) in dst_blocks.iter_mut().zip(src_blocks).zip(w_blocks) {
                let vs = vld2q_f32(src.as_ptr().cast());
                let vw = vld1q_f32(w.as_ptr());
                vst2q_f32(
                    dst.as_mut_ptr().cast(),
                    float32x4x2_t(vmulq_f32(vs.0, vw), vmulq_f32(vs.1, vw)),
                );
            }
        }
        scalar::scale_real_into(dst_tail, src_tail, w_tail);
    }

    pub(super) unsafe fn magnitudes_raw(iq: &[Complex<f32>], dst: *mut f32) {
        let (blocks, rest) = iq.as_chunks::<8>();
        let mut offset = 0;
        unsafe {
            for block in blocks {
                let p = block.as_ptr().cast::<f32>();
                let v0 = vld2q_f32(p);
                let v1 = vld2q_f32(p.add(8));
                let m0 = vsqrtq_f32(vfmaq_f32(vmulq_f32(v0.1, v0.1), v0.0, v0.0));
                let m1 = vsqrtq_f32(vfmaq_f32(vmulq_f32(v1.1, v1.1), v1.0, v1.0));
                vst1q_f32(dst.add(offset), m0);
                vst1q_f32(dst.add(offset + 4), m1);
                offset += 8;
            }
            let (quads, tail) = rest.as_chunks::<4>();
            for quad in quads {
                let v = vld2q_f32(quad.as_ptr().cast());
                let m = vsqrtq_f32(vfmaq_f32(vmulq_f32(v.1, v.1), v.0, v.0));
                vst1q_f32(dst.add(offset), m);
                offset += 4;
            }
            scalar::magnitudes_raw(tail, dst.add(offset));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_vec(len: usize, seed: usize) -> Vec<Complex<f32>> {
        (0..len)
            .map(|i| {
                let a = ((i * 37 + seed * 13) % 251) as f32 / 125.0 - 1.0;
                let b = ((i * 71 + seed * 29) % 257) as f32 / 128.0 - 1.0;
                Complex::new(a, b)
            })
            .collect()
    }

    #[test]
    fn complex_simd_primitives_match_scalar() {
        for len in [0, 1, 3, 4, 7, 8, 15, 31, 137] {
            let a = sample_vec(len, 1);
            let b = sample_vec(len, 2);
            let w: Vec<f32> = (0..len).map(|i| (i as f32 * 0.13).cos()).collect();

            let cs_got = cross_sum(&a, &b);
            let cs_ref = scalar::cross_sum(&a, &b);
            assert!((cs_got - cs_ref).norm() <= 1e-4 * (len as f64 + 1.0));

            let cd_got = conj_dot(&a, &b);
            let cd_ref = scalar::conj_dot(&a, &b);
            assert!((cd_got - cd_ref).norm() <= 1e-4 * (len as f32 + 1.0));

            let d_got = dot(&a, &b);
            let d_ref = scalar::dot(&a, &b);
            assert!((d_got - d_ref).norm() <= 1e-4 * (len as f32 + 1.0));

            let (mut acc_got, mut acc_ref) = (sample_vec(len, 3), sample_vec(len, 3));
            decay_cross_add(&mut acc_got, 0.85, &a, &b);
            scalar::decay_cross_add(&mut acc_ref, 0.85, &a, &b);
            for (g, r) in acc_got.iter().zip(&acc_ref) {
                assert!((g - r).norm() <= 1e-5);
            }

            let mut c64_got = vec![Complex::new(0.25, -0.5); len];
            let mut c64_ref = c64_got.clone();
            add_cross_f64(&mut c64_got, &a, &b);
            scalar::add_cross_f64(&mut c64_ref, &a, &b);
            for (g, r) in c64_got.iter().zip(&c64_ref) {
                assert!((g - r).norm() <= 1e-12);
            }

            let mut f64_got = vec![1.25f64; len];
            let mut f64_ref = f64_got.clone();
            add_norm_sqr_f64(&mut f64_got, &a);
            scalar::add_norm_sqr_f64(&mut f64_ref, &a);
            for (g, r) in f64_got.iter().zip(&f64_ref) {
                assert!((g - r).abs() <= 1e-12);
            }

            for turn in [Complex::new(1.0, 0.0), Complex::from_polar(1.0, 0.73)] {
                let mut r_got = vec![Complex::new(-0.1, 0.3); len];
                let mut r_ref = r_got.clone();
                add_rotated_f64(&mut r_got, &a, turn);
                scalar::add_rotated_f64(&mut r_ref, &a, turn);
                for (g, r) in r_got.iter().zip(&r_ref) {
                    assert!((g - r).norm() <= 1e-12);
                }
            }

            let gain = Complex::new(0.25, -0.75);
            let (mut h_got, mut h_ref) = (sample_vec(len, 4), sample_vec(len, 4));
            let n_got = axpy_norm_sqr(&mut h_got, &a, gain);
            let n_ref = scalar::axpy_norm_sqr(&mut h_ref, &a, gain);
            assert!((n_got - n_ref).abs() <= 1e-4 * (len as f32 + 1.0));
            for (g, r) in h_got.iter().zip(&h_ref) {
                assert!((g - r).norm() <= 1e-5);
            }

            let (mut mc_got, mut mc_ref) = (a.clone(), a.clone());
            mul_conj_in_place(&mut mc_got, &b);
            scalar::mul_conj_in_place(&mut mc_ref, &b);
            for (g, r) in mc_got.iter().zip(&mc_ref) {
                assert!((g - r).norm() <= 1e-5);
            }

            let ns_got = sum_norm_sqr(&a);
            let ns_ref = scalar::sum_norm_sqr(&a);
            assert!((ns_got - ns_ref).abs() <= 1e-4 * (len as f32 + 1.0));

            let (mut sc_got, mut sc_ref) =
                (vec![Complex::default(); len], vec![Complex::default(); len]);
            scale_real_into(&mut sc_got, &a, &w);
            scalar::scale_real_into(&mut sc_ref, &a, &w);
            assert_eq!(sc_got, sc_ref);

            let mut mag_got = Vec::new();
            magnitudes_into(&a, &mut mag_got);
            assert_eq!(mag_got.len(), len);
            for (s, &m) in a.iter().zip(&mag_got) {
                assert!((m - s.norm()).abs() <= 1e-6);
            }
        }
    }
}

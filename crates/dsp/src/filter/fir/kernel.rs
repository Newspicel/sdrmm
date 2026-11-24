use std::ops::Add;

use num_complex::Complex;

use super::plan::Plan;
use crate::math::fastmath::madd;

#[cfg(target_arch = "aarch64")]
pub(crate) use super::neon::Isa;
#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
pub(crate) use super::scalar::Isa;
#[cfg(target_arch = "x86_64")]
pub(crate) use super::x86::Isa;

pub(super) const BLOCK_FLOATS: usize = 16;

/// Blocks a kernel may compute side by side. The output's ragged end is computed as one such
/// group and copied out, so a delay line keeps this many blocks of slack.
pub(crate) const GROUP_BLOCKS: usize = 4;
const GROUP_FLOATS: usize = GROUP_BLOCKS * BLOCK_FLOATS;

pub(crate) trait Sample: Copy + Add<Output = Self> {
    const LANES: usize;
    const FLOATS: usize;
    fn zero() -> Self;
    fn deinterleave_2(src: &[[Self; 2]], r0: &mut [Self], r1: &mut [Self]) -> usize {
        let _ = (src, r0, r1);
        0
    }
    fn deinterleave_4(
        src: &[[Self; 4]],
        r0: &mut [Self],
        r1: &mut [Self],
        r2: &mut [Self],
        r3: &mut [Self],
    ) -> usize {
        let _ = (src, r0, r1, r2, r3);
        0
    }
}

impl Sample for f32 {
    const LANES: usize = BLOCK_FLOATS;
    const FLOATS: usize = 1;

    fn zero() -> Self {
        0.0
    }

    #[cfg(target_arch = "aarch64")]
    fn deinterleave_2(src: &[[Self; 2]], r0: &mut [Self], r1: &mut [Self]) -> usize {
        use std::arch::aarch64::{vld2q_f32, vst1q_f32};
        let n = src.len().min(r0.len()).min(r1.len()) & !3;
        unsafe {
            let sp = src.as_ptr().cast::<Self>();
            let d0 = r0.as_mut_ptr();
            let d1 = r1.as_mut_ptr();
            let mut i = 0;
            while i < n {
                let v = vld2q_f32(sp.add(i * 2));
                vst1q_f32(d0.add(i), v.0);
                vst1q_f32(d1.add(i), v.1);
                i += 4;
            }
        }
        n
    }

    #[cfg(target_arch = "aarch64")]
    fn deinterleave_4(
        src: &[[Self; 4]],
        r0: &mut [Self],
        r1: &mut [Self],
        r2: &mut [Self],
        r3: &mut [Self],
    ) -> usize {
        use std::arch::aarch64::{vld4q_f32, vst1q_f32};
        let n = src
            .len()
            .min(r0.len())
            .min(r1.len())
            .min(r2.len())
            .min(r3.len())
            & !3;
        unsafe {
            let sp = src.as_ptr().cast::<Self>();
            let (d0, d1, d2, d3) = (
                r0.as_mut_ptr(),
                r1.as_mut_ptr(),
                r2.as_mut_ptr(),
                r3.as_mut_ptr(),
            );
            let mut i = 0;
            while i < n {
                let v = vld4q_f32(sp.add(i * 4));
                vst1q_f32(d0.add(i), v.0);
                vst1q_f32(d1.add(i), v.1);
                vst1q_f32(d2.add(i), v.2);
                vst1q_f32(d3.add(i), v.3);
                i += 4;
            }
        }
        n
    }

    #[cfg(target_arch = "x86_64")]
    fn deinterleave_2(src: &[[Self; 2]], r0: &mut [Self], r1: &mut [Self]) -> usize {
        use std::arch::x86_64::{_mm_loadu_ps, _mm_shuffle_ps, _mm_storeu_ps};
        let n = src.len().min(r0.len()).min(r1.len()) & !3;
        unsafe {
            let sp = src.as_ptr().cast::<f32>();
            let d0 = r0.as_mut_ptr();
            let d1 = r1.as_mut_ptr();
            let mut i = 0;
            while i < n {
                let a = _mm_loadu_ps(sp.add(i * 2));
                let b = _mm_loadu_ps(sp.add(i * 2 + 4));
                _mm_storeu_ps(d0.add(i), _mm_shuffle_ps::<0x88>(a, b));
                _mm_storeu_ps(d1.add(i), _mm_shuffle_ps::<0xDD>(a, b));
                i += 4;
            }
        }
        n
    }

    #[cfg(target_arch = "x86_64")]
    fn deinterleave_4(
        src: &[[Self; 4]],
        r0: &mut [Self],
        r1: &mut [Self],
        r2: &mut [Self],
        r3: &mut [Self],
    ) -> usize {
        use std::arch::x86_64::{
            _mm_loadu_ps, _mm_movehl_ps, _mm_movelh_ps, _mm_storeu_ps, _mm_unpackhi_ps,
            _mm_unpacklo_ps,
        };
        let n = src
            .len()
            .min(r0.len())
            .min(r1.len())
            .min(r2.len())
            .min(r3.len())
            & !3;
        unsafe {
            let sp = src.as_ptr().cast::<f32>();
            let (d0, d1, d2, d3) = (
                r0.as_mut_ptr(),
                r1.as_mut_ptr(),
                r2.as_mut_ptr(),
                r3.as_mut_ptr(),
            );
            let mut i = 0;
            while i < n {
                let v0 = _mm_loadu_ps(sp.add(i * 4));
                let v1 = _mm_loadu_ps(sp.add(i * 4 + 4));
                let v2 = _mm_loadu_ps(sp.add(i * 4 + 8));
                let v3 = _mm_loadu_ps(sp.add(i * 4 + 12));
                let t0 = _mm_unpacklo_ps(v0, v1);
                let t1 = _mm_unpackhi_ps(v0, v1);
                let t2 = _mm_unpacklo_ps(v2, v3);
                let t3 = _mm_unpackhi_ps(v2, v3);
                _mm_storeu_ps(d0.add(i), _mm_movelh_ps(t0, t2));
                _mm_storeu_ps(d1.add(i), _mm_movehl_ps(t2, t0));
                _mm_storeu_ps(d2.add(i), _mm_movelh_ps(t1, t3));
                _mm_storeu_ps(d3.add(i), _mm_movehl_ps(t3, t1));
                i += 4;
            }
        }
        n
    }
}

impl Sample for Complex<f32> {
    const LANES: usize = BLOCK_FLOATS / 2;
    const FLOATS: usize = 2;

    fn zero() -> Self {
        Self::new(0.0, 0.0)
    }

    #[cfg(target_arch = "aarch64")]
    fn deinterleave_2(src: &[[Self; 2]], r0: &mut [Self], r1: &mut [Self]) -> usize {
        use std::arch::aarch64::{
            vld1q_f32, vreinterpretq_f32_u64, vreinterpretq_u64_f32, vst1q_f32, vuzp1q_u64,
            vuzp2q_u64,
        };
        let n = src.len().min(r0.len()).min(r1.len()) & !1;
        unsafe {
            let sp = src.as_ptr().cast::<f32>();
            let d0 = r0.as_mut_ptr().cast::<f32>();
            let d1 = r1.as_mut_ptr().cast::<f32>();
            let mut i = 0;
            while i < n {
                let a = vreinterpretq_u64_f32(vld1q_f32(sp.add(i * 4)));
                let b = vreinterpretq_u64_f32(vld1q_f32(sp.add(i * 4 + 4)));
                vst1q_f32(d0.add(i * 2), vreinterpretq_f32_u64(vuzp1q_u64(a, b)));
                vst1q_f32(d1.add(i * 2), vreinterpretq_f32_u64(vuzp2q_u64(a, b)));
                i += 2;
            }
        }
        n
    }

    #[cfg(target_arch = "aarch64")]
    fn deinterleave_4(
        src: &[[Self; 4]],
        r0: &mut [Self],
        r1: &mut [Self],
        r2: &mut [Self],
        r3: &mut [Self],
    ) -> usize {
        use std::arch::aarch64::{
            vld1q_f32, vreinterpretq_f32_u64, vreinterpretq_u64_f32, vst1q_f32, vuzp1q_u64,
            vuzp2q_u64,
        };
        let n = src
            .len()
            .min(r0.len())
            .min(r1.len())
            .min(r2.len())
            .min(r3.len())
            & !1;
        unsafe {
            let sp = src.as_ptr().cast::<f32>();
            let (d0, d1, d2, d3) = (
                r0.as_mut_ptr().cast::<f32>(),
                r1.as_mut_ptr().cast::<f32>(),
                r2.as_mut_ptr().cast::<f32>(),
                r3.as_mut_ptr().cast::<f32>(),
            );
            let mut i = 0;
            while i < n {
                let a0 = vreinterpretq_u64_f32(vld1q_f32(sp.add(i * 8)));
                let a1 = vreinterpretq_u64_f32(vld1q_f32(sp.add(i * 8 + 4)));
                let b0 = vreinterpretq_u64_f32(vld1q_f32(sp.add(i * 8 + 8)));
                let b1 = vreinterpretq_u64_f32(vld1q_f32(sp.add(i * 8 + 12)));
                vst1q_f32(d0.add(i * 2), vreinterpretq_f32_u64(vuzp1q_u64(a0, b0)));
                vst1q_f32(d1.add(i * 2), vreinterpretq_f32_u64(vuzp2q_u64(a0, b0)));
                vst1q_f32(d2.add(i * 2), vreinterpretq_f32_u64(vuzp1q_u64(a1, b1)));
                vst1q_f32(d3.add(i * 2), vreinterpretq_f32_u64(vuzp2q_u64(a1, b1)));
                i += 2;
            }
        }
        n
    }

    #[cfg(target_arch = "x86_64")]
    fn deinterleave_2(src: &[[Self; 2]], r0: &mut [Self], r1: &mut [Self]) -> usize {
        use std::arch::x86_64::{_mm_loadu_ps, _mm_movehl_ps, _mm_movelh_ps, _mm_storeu_ps};
        let n = src.len().min(r0.len()).min(r1.len()) & !1;
        unsafe {
            let sp = src.as_ptr().cast::<f32>();
            let d0 = r0.as_mut_ptr().cast::<f32>();
            let d1 = r1.as_mut_ptr().cast::<f32>();
            let mut i = 0;
            while i < n {
                let a = _mm_loadu_ps(sp.add(i * 4));
                let b = _mm_loadu_ps(sp.add(i * 4 + 4));
                _mm_storeu_ps(d0.add(i * 2), _mm_movelh_ps(a, b));
                _mm_storeu_ps(d1.add(i * 2), _mm_movehl_ps(b, a));
                i += 2;
            }
        }
        n
    }

    #[cfg(target_arch = "x86_64")]
    fn deinterleave_4(
        src: &[[Self; 4]],
        r0: &mut [Self],
        r1: &mut [Self],
        r2: &mut [Self],
        r3: &mut [Self],
    ) -> usize {
        use std::arch::x86_64::{_mm_loadu_ps, _mm_movehl_ps, _mm_movelh_ps, _mm_storeu_ps};
        let n = src
            .len()
            .min(r0.len())
            .min(r1.len())
            .min(r2.len())
            .min(r3.len())
            & !1;
        unsafe {
            let sp = src.as_ptr().cast::<f32>();
            let (d0, d1, d2, d3) = (
                r0.as_mut_ptr().cast::<f32>(),
                r1.as_mut_ptr().cast::<f32>(),
                r2.as_mut_ptr().cast::<f32>(),
                r3.as_mut_ptr().cast::<f32>(),
            );
            let mut i = 0;
            while i < n {
                let a0 = _mm_loadu_ps(sp.add(i * 8));
                let a1 = _mm_loadu_ps(sp.add(i * 8 + 4));
                let b0 = _mm_loadu_ps(sp.add(i * 8 + 8));
                let b1 = _mm_loadu_ps(sp.add(i * 8 + 12));
                _mm_storeu_ps(d0.add(i * 2), _mm_movelh_ps(a0, b0));
                _mm_storeu_ps(d1.add(i * 2), _mm_movehl_ps(b0, a0));
                _mm_storeu_ps(d2.add(i * 2), _mm_movelh_ps(a1, b1));
                _mm_storeu_ps(d3.add(i * 2), _mm_movehl_ps(b1, a1));
                i += 2;
            }
        }
        n
    }
}

pub(crate) trait Accumulate<C>: Sample {
    fn add_product(self, sample: Self, coefficient: C) -> Self;
    /// Fills `out` with consecutive outputs, the first from the window at the start of `window`.
    fn blocks<const FOLD: bool>(isa: Isa, window: &[Self], plan: &Plan<C>, out: &mut [Self]);
}

impl Accumulate<Self> for f32 {
    fn add_product(self, sample: Self, coefficient: Self) -> Self {
        madd(sample, coefficient, self)
    }

    fn blocks<const FOLD: bool>(isa: Isa, window: &[Self], plan: &Plan<Self>, out: &mut [Self]) {
        let whole = out.len() - out.len() % (Self::LANES * GROUP_BLOCKS);
        let (full, tail) = out.split_at_mut(whole);
        isa.real_blocks::<FOLD>(window, plan, full);
        if !tail.is_empty() {
            let mut lanes = [0.0; GROUP_FLOATS];
            isa.real_blocks::<FOLD>(&window[whole..], plan, &mut lanes);
            tail.copy_from_slice(&lanes[..tail.len()]);
        }
    }
}

impl Accumulate<f32> for Complex<f32> {
    fn add_product(self, sample: Self, coefficient: f32) -> Self {
        Self::new(
            madd(sample.re, coefficient, self.re),
            madd(sample.im, coefficient, self.im),
        )
    }

    fn blocks<const FOLD: bool>(isa: Isa, window: &[Self], plan: &Plan<f32>, out: &mut [Self]) {
        let whole = out.len() - out.len() % (Self::LANES * GROUP_BLOCKS);
        let (full, tail) = out.split_at_mut(whole);
        isa.complex_real_blocks::<FOLD>(floats(window), plan, floats_mut(full));
        if !tail.is_empty() {
            let mut lanes = [0.0; GROUP_FLOATS];
            isa.complex_real_blocks::<FOLD>(floats(&window[whole..]), plan, &mut lanes);
            unpack(&lanes, tail);
        }
    }
}

impl Accumulate<Self> for Complex<f32> {
    fn add_product(self, sample: Self, coefficient: Self) -> Self {
        Self::new(
            madd(
                -sample.im,
                coefficient.im,
                madd(sample.re, coefficient.re, self.re),
            ),
            madd(
                sample.im,
                coefficient.re,
                madd(sample.re, coefficient.im, self.im),
            ),
        )
    }

    fn blocks<const FOLD: bool>(isa: Isa, window: &[Self], plan: &Plan<Self>, out: &mut [Self]) {
        let whole = out.len() - out.len() % (Self::LANES * GROUP_BLOCKS);
        let (full, tail) = out.split_at_mut(whole);
        isa.complex_blocks::<FOLD>(floats(window), plan, floats_mut(full));
        if !tail.is_empty() {
            let mut lanes = [0.0; GROUP_FLOATS];
            isa.complex_blocks::<FOLD>(floats(&window[whole..]), plan, &mut lanes);
            unpack(&lanes, tail);
        }
    }
}

pub(super) fn floats(samples: &[Complex<f32>]) -> &[f32] {
    unsafe { std::slice::from_raw_parts(samples.as_ptr().cast(), 2 * samples.len()) }
}

fn floats_mut(samples: &mut [Complex<f32>]) -> &mut [f32] {
    unsafe { std::slice::from_raw_parts_mut(samples.as_mut_ptr().cast(), 2 * samples.len()) }
}

fn unpack(lanes: &[f32], out: &mut [Complex<f32>]) {
    for (sample, pair) in out.iter_mut().zip(lanes.as_chunks::<2>().0) {
        *sample = Complex::new(pair[0], pair[1]);
    }
}

#[cfg(test)]
#[inline(always)]
pub(crate) fn plane_dot<const N: usize>(
    isa: Isa,
    re: &[f32],
    im: &[f32],
    starts: [usize; N],
    taps: &[f32],
) -> [Complex<f32>; N] {
    let len = taps.len();
    let bound = re.len().min(im.len());
    let max_end = starts.into_iter().fold(0usize, |acc, s| acc.max(s + len));
    assert!(max_end <= bound, "plane_dot window past delay line");
    let re_ptrs = std::array::from_fn(|i| unsafe { re.as_ptr().add(starts[i]) });
    let im_ptrs = std::array::from_fn(|i| unsafe { im.as_ptr().add(starts[i]) });
    unsafe { isa.plane_dot(re_ptrs, im_ptrs, taps) }
}

pub(crate) fn plane_interpolated(
    isa: Isa,
    re: &[f32],
    im: &[f32],
    lower: &[f32],
    slope: &[f32],
    mu: f32,
) -> Complex<f32> {
    let len = lower.len().min(slope.len()).min(re.len()).min(im.len());
    isa.plane_interpolated(&re[..len], &im[..len], &lower[..len], &slope[..len], mu)
}

pub(super) unsafe fn plane_tail(
    sum: Complex<f32>,
    re: *const f32,
    im: *const f32,
    taps: &[f32],
    from: usize,
) -> Complex<f32> {
    taps.iter()
        .enumerate()
        .skip(from)
        .fold(sum, |sum, (index, &tap)| {
            let sample = unsafe { Complex::new(*re.add(index), *im.add(index)) };
            sum.add_product(sample, tap)
        })
}

pub(super) fn interpolated_plane_tail(
    sum: Complex<f32>,
    re: &[f32],
    im: &[f32],
    lower: &[f32],
    slope: &[f32],
    mu: f32,
) -> Complex<f32> {
    re.iter().zip(im).zip(lower.iter().zip(slope)).fold(
        sum,
        |sum, ((&re, &im), (&lower, &slope))| {
            sum.add_product(Complex::new(re, im), madd(slope, mu, lower))
        },
    )
}

#[cfg(test)]
mod tests;

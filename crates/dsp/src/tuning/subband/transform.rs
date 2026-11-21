use num_complex::Complex;

const RADIX: usize = 5;
const LEN: usize = RADIX * RADIX;
/// `cos(2 pi / 5)`, `-cos(4 pi / 5)`, `sin(2 pi / 5)` and `sin(4 pi / 5)`.
const COS_1: f32 = 0.309_017;
const NEG_COS_2: f32 = 0.809_017;
const SIN_1: f32 = 0.951_056_54;
const SIN_2: f32 = 0.587_785_24;

pub(super) struct Inverse25 {
    twiddles: [[Complex<f32>; RADIX - 1]; RADIX - 1],
}

impl Inverse25 {
    pub(super) fn new() -> Self {
        Self {
            twiddles: std::array::from_fn(|row| {
                std::array::from_fn(|column| {
                    let angle =
                        std::f64::consts::TAU * ((row + 1) * (column + 1)) as f64 / LEN as f64;
                    let (sin, cos) = angle.sin_cos();
                    Complex::new(cos as f32, sin as f32)
                })
            }),
        }
    }

    pub(super) fn process(&self, input: &mut [Complex<f32>; LEN]) {
        let mut columns = [[Complex::new(0.0, 0.0); RADIX]; RADIX];
        for (row, column) in columns.iter_mut().enumerate() {
            *column = inverse5(std::array::from_fn(|index| input[RADIX * index + row]));
            if row > 0 {
                for (value, twiddle) in column[1..].iter_mut().zip(self.twiddles[row - 1]) {
                    *value *= twiddle;
                }
            }
        }
        for column in 0..RADIX {
            let result = inverse5(std::array::from_fn(|row| columns[row][column]));
            for (row, value) in result.into_iter().enumerate() {
                input[RADIX * row + column] = value;
            }
        }
    }

    #[cfg(target_arch = "aarch64")]
    #[inline(never)]
    pub(super) unsafe fn process_4_subbands(
        &self,
        sums_re: &[[f32; LEN]; 4],
        sums_im: &[[f32; LEN]; 4],
        phases: [usize; 4],
        rot_src: &[[usize; LEN]; RADIX],
        out_ptrs: &[*mut f32; super::SUBBANDS],
        out_offset: usize,
    ) {
        use std::arch::aarch64::{
            vfmaq_laneq_f32, vfmsq_laneq_f32, vld1q_dup_f32, vld1q_f32, vld1q_lane_f32,
            vmulq_laneq_f32, vst1q_f32,
        };
        let consts_arr = [COS_1, NEG_COS_2, SIN_1, SIN_2];
        let consts = unsafe { vld1q_f32(consts_arr.as_ptr()) };
        let mut col_re = [[[0.0f32; 4]; RADIX]; RADIX];
        let mut col_im = [[[0.0f32; 4]; RADIX]; RADIX];
        let (r0, r1, r2, r3) = (
            sums_re[0].as_ptr(),
            sums_re[1].as_ptr(),
            sums_re[2].as_ptr(),
            sums_re[3].as_ptr(),
        );
        let (im0, im1, im2, im3) = (
            sums_im[0].as_ptr(),
            sums_im[1].as_ptr(),
            sums_im[2].as_ptr(),
            sums_im[3].as_ptr(),
        );
        let (p0, p1, p2, p3) = (
            &rot_src[phases[0]],
            &rot_src[phases[1]],
            &rot_src[phases[2]],
            &rot_src[phases[3]],
        );
        for row in 0..RADIX {
            let mut in_re = [consts; RADIX];
            let mut in_im = [consts; RADIX];
            for i in 0..RADIX {
                let dst = RADIX * i + row;
                let (i0, i1, i2, i3) = (p0[dst], p1[dst], p2[dst], p3[dst]);
                unsafe {
                    let vr = vld1q_dup_f32(r0.add(i0));
                    let vr = vld1q_lane_f32::<1>(r1.add(i1), vr);
                    let vr = vld1q_lane_f32::<2>(r2.add(i2), vr);
                    in_re[i] = vld1q_lane_f32::<3>(r3.add(i3), vr);

                    let vi = vld1q_dup_f32(im0.add(i0));
                    let vi = vld1q_lane_f32::<1>(im1.add(i1), vi);
                    let vi = vld1q_lane_f32::<2>(im2.add(i2), vi);
                    in_im[i] = vld1q_lane_f32::<3>(im3.add(i3), vi);
                }
            }
            let (mut out_re, mut out_im) = inverse5_neon(in_re, in_im, consts);
            if row > 0 {
                let tw = &self.twiddles[row - 1];
                let pair0 = [tw[0].re, tw[0].im, tw[1].re, tw[1].im];
                let pair1 = [tw[2].re, tw[2].im, tw[3].re, tw[3].im];
                unsafe {
                    let t01 = vld1q_f32(pair0.as_ptr());
                    let t23 = vld1q_f32(pair1.as_ptr());
                    let (a1, b1) = (out_re[1], out_im[1]);
                    out_re[1] = vfmsq_laneq_f32::<1>(vmulq_laneq_f32::<0>(a1, t01), b1, t01);
                    out_im[1] = vfmaq_laneq_f32::<0>(vmulq_laneq_f32::<1>(a1, t01), b1, t01);
                    let (a2, b2) = (out_re[2], out_im[2]);
                    out_re[2] = vfmsq_laneq_f32::<3>(vmulq_laneq_f32::<2>(a2, t01), b2, t01);
                    out_im[2] = vfmaq_laneq_f32::<2>(vmulq_laneq_f32::<3>(a2, t01), b2, t01);
                    let (a3, b3) = (out_re[3], out_im[3]);
                    out_re[3] = vfmsq_laneq_f32::<1>(vmulq_laneq_f32::<0>(a3, t23), b3, t23);
                    out_im[3] = vfmaq_laneq_f32::<0>(vmulq_laneq_f32::<1>(a3, t23), b3, t23);
                    let (a4, b4) = (out_re[4], out_im[4]);
                    out_re[4] = vfmsq_laneq_f32::<3>(vmulq_laneq_f32::<2>(a4, t23), b4, t23);
                    out_im[4] = vfmaq_laneq_f32::<2>(vmulq_laneq_f32::<3>(a4, t23), b4, t23);
                }
            }
            for col in 0..RADIX {
                unsafe {
                    vst1q_f32(col_re[col][row].as_mut_ptr(), out_re[col]);
                    vst1q_f32(col_im[col][row].as_mut_ptr(), out_im[col]);
                }
            }
        }
        unsafe {
            store_column::<0>(&col_re, &col_im, consts, out_ptrs, out_offset);
            store_column::<1>(&col_re, &col_im, consts, out_ptrs, out_offset);
            store_column::<2>(&col_re, &col_im, consts, out_ptrs, out_offset);
            store_column::<3>(&col_re, &col_im, consts, out_ptrs, out_offset);
            store_column::<4>(&col_re, &col_im, consts, out_ptrs, out_offset);
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2", enable = "fma")]
    pub(super) unsafe fn process_8(
        &self,
        re: &mut [std::arch::x86_64::__m256; LEN],
        im: &mut [std::arch::x86_64::__m256; LEN],
    ) {
        use std::arch::x86_64::{
            __m256, _mm256_fmadd_ps, _mm256_fmsub_ps, _mm256_mul_ps, _mm256_set1_ps,
            _mm256_setzero_ps,
        };
        let zero = _mm256_setzero_ps();
        let mut col_re = [[zero; RADIX]; RADIX];
        let mut col_im = [[zero; RADIX]; RADIX];
        for row in 0..RADIX {
            let in_re = std::array::from_fn(|i| re[RADIX * i + row]);
            let in_im = std::array::from_fn(|i| im[RADIX * i + row]);
            let (mut out_re, mut out_im) = unsafe { inverse5_avx2(in_re, in_im) };
            if row > 0 {
                for (k, twiddle) in self.twiddles[row - 1].iter().enumerate() {
                    let r = out_re[k + 1];
                    let i = out_im[k + 1];
                    let tr = _mm256_set1_ps(twiddle.re);
                    let ti = _mm256_set1_ps(twiddle.im);
                    out_re[k + 1] = _mm256_fmsub_ps(r, tr, _mm256_mul_ps(i, ti));
                    out_im[k + 1] = _mm256_fmadd_ps(r, ti, _mm256_mul_ps(i, tr));
                }
            }
            col_re[row] = out_re;
            col_im[row] = out_im;
        }
        for column in 0..RADIX {
            let in_re: [__m256; RADIX] = std::array::from_fn(|row| col_re[row][column]);
            let in_im: [__m256; RADIX] = std::array::from_fn(|row| col_im[row][column]);
            let (out_re, out_im) = unsafe { inverse5_avx2(in_re, in_im) };
            for row in 0..RADIX {
                re[RADIX * row + column] = out_re[row];
                im[RADIX * row + column] = out_im[row];
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "fma")]
#[inline]
unsafe fn inverse5_avx2(
    re: [std::arch::x86_64::__m256; RADIX],
    im: [std::arch::x86_64::__m256; RADIX],
) -> (
    [std::arch::x86_64::__m256; RADIX],
    [std::arch::x86_64::__m256; RADIX],
) {
    use std::arch::x86_64::{
        _mm256_add_ps, _mm256_fmadd_ps, _mm256_fnmadd_ps, _mm256_mul_ps, _mm256_set1_ps,
        _mm256_sub_ps,
    };
    let cos1 = _mm256_set1_ps(COS_1);
    let ncos2 = _mm256_set1_ps(NEG_COS_2);
    let sin1 = _mm256_set1_ps(SIN_1);
    let sin2 = _mm256_set1_ps(SIN_2);

    let p1_re = _mm256_add_ps(re[1], re[4]);
    let p1_im = _mm256_add_ps(im[1], im[4]);
    let p2_re = _mm256_add_ps(re[2], re[3]);
    let p2_im = _mm256_add_ps(im[2], im[3]);
    let d1_re = _mm256_sub_ps(re[1], re[4]);
    let d1_im = _mm256_sub_ps(im[1], im[4]);
    let d2_re = _mm256_sub_ps(re[2], re[3]);
    let d2_im = _mm256_sub_ps(im[2], im[3]);

    let e1_re = _mm256_fnmadd_ps(p2_re, ncos2, _mm256_fmadd_ps(p1_re, cos1, re[0]));
    let e1_im = _mm256_fnmadd_ps(p2_im, ncos2, _mm256_fmadd_ps(p1_im, cos1, im[0]));
    let e2_re = _mm256_fmadd_ps(p2_re, cos1, _mm256_fnmadd_ps(p1_re, ncos2, re[0]));
    let e2_im = _mm256_fmadd_ps(p2_im, cos1, _mm256_fnmadd_ps(p1_im, ncos2, im[0]));

    let o1_re = _mm256_fmadd_ps(d2_re, sin2, _mm256_mul_ps(d1_re, sin1));
    let o1_im = _mm256_fmadd_ps(d2_im, sin2, _mm256_mul_ps(d1_im, sin1));
    let o2_re = _mm256_fnmadd_ps(d2_re, sin1, _mm256_mul_ps(d1_re, sin2));
    let o2_im = _mm256_fnmadd_ps(d2_im, sin1, _mm256_mul_ps(d1_im, sin2));

    (
        [
            _mm256_add_ps(re[0], _mm256_add_ps(p1_re, p2_re)),
            _mm256_sub_ps(e1_re, o1_im),
            _mm256_sub_ps(e2_re, o2_im),
            _mm256_add_ps(e2_re, o2_im),
            _mm256_add_ps(e1_re, o1_im),
        ],
        [
            _mm256_add_ps(im[0], _mm256_add_ps(p1_im, p2_im)),
            _mm256_add_ps(e1_im, o1_re),
            _mm256_add_ps(e2_im, o2_re),
            _mm256_sub_ps(e2_im, o2_re),
            _mm256_sub_ps(e1_im, o1_re),
        ],
    )
}

/// The band each bin of the transform feeds, if any.
#[cfg(target_arch = "aarch64")]
const BIN_BAND: [Option<usize>; LEN] = {
    let mut table = [None; LEN];
    let mut band = 0;
    while band < super::SUBBANDS {
        table[super::bank::BAND_BINS[band]] = Some(band);
        band += 1;
    }
    table
};

/// Finishes one column of the transform, writing only the rows a band reads.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn store_column<const COLUMN: usize>(
    col_re: &[[[f32; 4]; RADIX]; RADIX],
    col_im: &[[[f32; 4]; RADIX]; RADIX],
    consts: std::arch::aarch64::float32x4_t,
    out_ptrs: &[*mut f32; super::SUBBANDS],
    out_offset: usize,
) {
    use std::arch::aarch64::{float32x4x2_t, vld1q_f32, vst2q_f32};
    unsafe {
        let fifth = Fifth::new(
            std::array::from_fn(|row| vld1q_f32(col_re[COLUMN][row].as_ptr())),
            std::array::from_fn(|row| vld1q_f32(col_im[COLUMN][row].as_ptr())),
        );
        for row in 0..RADIX {
            if let Some(band) = BIN_BAND[RADIX * row + COLUMN] {
                let (re, im) = fifth.row(row, consts);
                vst2q_f32(out_ptrs[band].add(out_offset), float32x4x2_t(re, im));
            }
        }
    }
}

/// The sums and differences every output of a five-point inverse transform shares.
#[cfg(target_arch = "aarch64")]
struct Fifth {
    re0: std::arch::aarch64::float32x4_t,
    im0: std::arch::aarch64::float32x4_t,
    p1_re: std::arch::aarch64::float32x4_t,
    p1_im: std::arch::aarch64::float32x4_t,
    p2_re: std::arch::aarch64::float32x4_t,
    p2_im: std::arch::aarch64::float32x4_t,
    d1_re: std::arch::aarch64::float32x4_t,
    d1_im: std::arch::aarch64::float32x4_t,
    d2_re: std::arch::aarch64::float32x4_t,
    d2_im: std::arch::aarch64::float32x4_t,
}

#[cfg(target_arch = "aarch64")]
impl Fifth {
    #[inline(always)]
    unsafe fn new(
        re: [std::arch::aarch64::float32x4_t; RADIX],
        im: [std::arch::aarch64::float32x4_t; RADIX],
    ) -> Self {
        use std::arch::aarch64::{vaddq_f32, vsubq_f32};
        unsafe {
            Self {
                re0: re[0],
                im0: im[0],
                p1_re: vaddq_f32(re[1], re[4]),
                p1_im: vaddq_f32(im[1], im[4]),
                p2_re: vaddq_f32(re[2], re[3]),
                p2_im: vaddq_f32(im[2], im[3]),
                d1_re: vsubq_f32(re[1], re[4]),
                d1_im: vsubq_f32(im[1], im[4]),
                d2_re: vsubq_f32(re[2], re[3]),
                d2_im: vsubq_f32(im[2], im[3]),
            }
        }
    }

    /// Output `row`; `consts` holds `COS_1`, `NEG_COS_2`, `SIN_1` and `SIN_2`.
    #[inline(always)]
    unsafe fn row(
        &self,
        row: usize,
        consts: std::arch::aarch64::float32x4_t,
    ) -> (
        std::arch::aarch64::float32x4_t,
        std::arch::aarch64::float32x4_t,
    ) {
        use std::arch::aarch64::{
            vaddq_f32, vfmaq_laneq_f32, vfmsq_laneq_f32, vmulq_laneq_f32, vsubq_f32,
        };
        let Self {
            re0,
            im0,
            p1_re,
            p1_im,
            p2_re,
            p2_im,
            d1_re,
            d1_im,
            d2_re,
            d2_im,
        } = *self;
        unsafe {
            let (e_re, e_im, o_re, o_im) = match row {
                0 => {
                    return (
                        vaddq_f32(re0, vaddq_f32(p1_re, p2_re)),
                        vaddq_f32(im0, vaddq_f32(p1_im, p2_im)),
                    );
                }
                1 | 4 => (
                    vfmsq_laneq_f32::<1>(vfmaq_laneq_f32::<0>(re0, p1_re, consts), p2_re, consts),
                    vfmsq_laneq_f32::<1>(vfmaq_laneq_f32::<0>(im0, p1_im, consts), p2_im, consts),
                    vfmaq_laneq_f32::<3>(vmulq_laneq_f32::<2>(d1_re, consts), d2_re, consts),
                    vfmaq_laneq_f32::<3>(vmulq_laneq_f32::<2>(d1_im, consts), d2_im, consts),
                ),
                _ => (
                    vfmaq_laneq_f32::<0>(vfmsq_laneq_f32::<1>(re0, p1_re, consts), p2_re, consts),
                    vfmaq_laneq_f32::<0>(vfmsq_laneq_f32::<1>(im0, p1_im, consts), p2_im, consts),
                    vfmsq_laneq_f32::<2>(vmulq_laneq_f32::<3>(d1_re, consts), d2_re, consts),
                    vfmsq_laneq_f32::<2>(vmulq_laneq_f32::<3>(d1_im, consts), d2_im, consts),
                ),
            };
            if row <= 2 {
                (vsubq_f32(e_re, o_im), vaddq_f32(e_im, o_re))
            } else {
                (vaddq_f32(e_re, o_im), vsubq_f32(e_im, o_re))
            }
        }
    }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn inverse5_neon(
    re: [std::arch::aarch64::float32x4_t; RADIX],
    im: [std::arch::aarch64::float32x4_t; RADIX],
    consts: std::arch::aarch64::float32x4_t,
) -> (
    [std::arch::aarch64::float32x4_t; RADIX],
    [std::arch::aarch64::float32x4_t; RADIX],
) {
    unsafe {
        let fifth = Fifth::new(re, im);
        let rows: [_; RADIX] = std::array::from_fn(|row| fifth.row(row, consts));
        (rows.map(|(re, _)| re), rows.map(|(_, im)| im))
    }
}

fn inverse5(input: [Complex<f32>; RADIX]) -> [Complex<f32>; RADIX] {
    let pair1 = input[1] + input[4];
    let pair2 = input[2] + input[3];
    let difference1 = input[1] - input[4];
    let difference2 = input[2] - input[3];
    let even1 = input[0] + pair1 * COS_1 - pair2 * NEG_COS_2;
    let even2 = input[0] - pair1 * NEG_COS_2 + pair2 * COS_1;
    let odd1 = difference1 * SIN_1 + difference2 * SIN_2;
    let odd2 = difference1 * SIN_2 - difference2 * SIN_1;
    let odd1 = Complex::new(-odd1.im, odd1.re);
    let odd2 = Complex::new(-odd2.im, odd2.re);
    [
        input[0] + pair1 + pair2,
        even1 + odd1,
        even2 + odd2,
        even2 - odd2,
        even1 - odd1,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::complex::widen;

    #[test]
    fn inverse_matches_double_precision_dft_for_impulses_and_noise() {
        let transform = Inverse25::new();
        for case in 0..125 {
            let input = std::array::from_fn(|index| {
                if case < 25 {
                    Complex::new(f32::from(index == case), 0.0)
                } else {
                    Complex::new(
                        ((index * 37 + case * 71) % 251) as f32 / 125.0 - 1.0,
                        ((index * 73 + case * 43) % 257) as f32 / 128.0 - 1.0,
                    )
                }
            });
            let mut actual = input;
            transform.process(&mut actual);
            for (bin, value) in actual.into_iter().enumerate() {
                let expected: Complex<f64> = input
                    .iter()
                    .enumerate()
                    .map(|(index, sample)| {
                        let angle = std::f64::consts::TAU * (index * bin) as f64 / 25.0;
                        widen(*sample) * Complex::from_polar(1.0, angle)
                    })
                    .sum();
                let error = (widen(value) - expected).norm();
                assert!(error < 3e-6, "case={case} bin={bin} error={error}");
            }
        }
    }
}

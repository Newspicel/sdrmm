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
    pub(super) fn process_4(
        &self,
        re: &mut [std::arch::aarch64::float32x4_t; LEN],
        im: &mut [std::arch::aarch64::float32x4_t; LEN],
    ) {
        use std::arch::aarch64::{float32x4_t, vdupq_n_f32, vfmaq_n_f32, vfmsq_n_f32, vmulq_n_f32};
        let zero = unsafe { vdupq_n_f32(0.0) };
        let mut col_re = [[zero; RADIX]; RADIX];
        let mut col_im = [[zero; RADIX]; RADIX];
        for row in 0..RADIX {
            let in_re = std::array::from_fn(|i| re[RADIX * i + row]);
            let in_im = std::array::from_fn(|i| im[RADIX * i + row]);
            let (mut out_re, mut out_im) = inverse5_neon(in_re, in_im);
            if row > 0 {
                for (k, twiddle) in self.twiddles[row - 1].iter().enumerate() {
                    let r = out_re[k + 1];
                    let i = out_im[k + 1];
                    unsafe {
                        out_re[k + 1] = vfmsq_n_f32(vmulq_n_f32(r, twiddle.re), i, twiddle.im);
                        out_im[k + 1] = vfmaq_n_f32(vmulq_n_f32(r, twiddle.im), i, twiddle.re);
                    }
                }
            }
            col_re[row] = out_re;
            col_im[row] = out_im;
        }
        for column in 0..RADIX {
            let in_re: [float32x4_t; RADIX] = std::array::from_fn(|row| col_re[row][column]);
            let in_im: [float32x4_t; RADIX] = std::array::from_fn(|row| col_im[row][column]);
            let (out_re, out_im) = inverse5_neon(in_re, in_im);
            for row in 0..RADIX {
                re[RADIX * row + column] = out_re[row];
                im[RADIX * row + column] = out_im[row];
            }
        }
    }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn inverse5_neon(
    re: [std::arch::aarch64::float32x4_t; RADIX],
    im: [std::arch::aarch64::float32x4_t; RADIX],
) -> (
    [std::arch::aarch64::float32x4_t; RADIX],
    [std::arch::aarch64::float32x4_t; RADIX],
) {
    use std::arch::aarch64::{vaddq_f32, vfmaq_n_f32, vfmsq_n_f32, vmulq_n_f32, vsubq_f32};
    unsafe {
        let p1_re = vaddq_f32(re[1], re[4]);
        let p1_im = vaddq_f32(im[1], im[4]);
        let p2_re = vaddq_f32(re[2], re[3]);
        let p2_im = vaddq_f32(im[2], im[3]);
        let d1_re = vsubq_f32(re[1], re[4]);
        let d1_im = vsubq_f32(im[1], im[4]);
        let d2_re = vsubq_f32(re[2], re[3]);
        let d2_im = vsubq_f32(im[2], im[3]);

        let e1_re = vfmsq_n_f32(vfmaq_n_f32(re[0], p1_re, COS_1), p2_re, NEG_COS_2);
        let e1_im = vfmsq_n_f32(vfmaq_n_f32(im[0], p1_im, COS_1), p2_im, NEG_COS_2);
        let e2_re = vfmaq_n_f32(vfmsq_n_f32(re[0], p1_re, NEG_COS_2), p2_re, COS_1);
        let e2_im = vfmaq_n_f32(vfmsq_n_f32(im[0], p1_im, NEG_COS_2), p2_im, COS_1);

        let o1_re = vfmaq_n_f32(vmulq_n_f32(d1_re, SIN_1), d2_re, SIN_2);
        let o1_im = vfmaq_n_f32(vmulq_n_f32(d1_im, SIN_1), d2_im, SIN_2);
        let o2_re = vfmsq_n_f32(vmulq_n_f32(d1_re, SIN_2), d2_re, SIN_1);
        let o2_im = vfmsq_n_f32(vmulq_n_f32(d1_im, SIN_2), d2_im, SIN_1);

        (
            [
                vaddq_f32(re[0], vaddq_f32(p1_re, p2_re)),
                vsubq_f32(e1_re, o1_im),
                vsubq_f32(e2_re, o2_im),
                vaddq_f32(e2_re, o2_im),
                vaddq_f32(e1_re, o1_im),
            ],
            [
                vaddq_f32(im[0], vaddq_f32(p1_im, p2_im)),
                vaddq_f32(e1_im, o1_re),
                vaddq_f32(e2_im, o2_re),
                vsubq_f32(e2_im, o2_re),
                vsubq_f32(e1_im, o1_re),
            ],
        )
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

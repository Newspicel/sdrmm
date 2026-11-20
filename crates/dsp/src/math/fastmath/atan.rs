use std::f32::consts::{FRAC_PI_2, PI};

use num_complex::Complex;

pub(super) const ATAN_UNIT: [f32; 7] = [
    0.999_996_1,
    -0.333_173_7,
    0.198_078_14,
    -0.132_333_38,
    0.079_623_6,
    -0.033_604_16,
    0.006_811_773,
];

#[inline(always)]
fn atan_unit(ratio: f32) -> f32 {
    let square = ratio * ratio;
    #[cfg(target_arch = "aarch64")]
    {
        let [c0, c1, c2, c3, c4, c5, c6] = ATAN_UNIT;
        let s2 = square * square;
        let s4 = s2 * s2;
        let p01 = square.mul_add(c1, c0);
        let p23 = square.mul_add(c3, c2);
        let p45 = square.mul_add(c5, c4);
        let p03 = s2.mul_add(p23, p01);
        let p46 = s2.mul_add(c6, p45);
        s4.mul_add(p46, p03) * ratio
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        let mut poly = ATAN_UNIT[6];
        poly = poly * square + ATAN_UNIT[5];
        poly = poly * square + ATAN_UNIT[4];
        poly = poly * square + ATAN_UNIT[3];
        poly = poly * square + ATAN_UNIT[2];
        poly = poly * square + ATAN_UNIT[1];
        poly = poly * square + ATAN_UNIT[0];
        poly * ratio
    }
}

#[inline(always)]
#[must_use]
pub fn fast_atan2(y: f32, x: f32) -> f32 {
    let (ax, ay) = (x.abs(), y.abs());
    let steep = ay > ax;
    let (lo, hi) = if steep { (ax, ay) } else { (ay, ax) };
    let ratio = if ax == ay { 1.0 } else { lo / hi };
    let mut angle = atan_unit(ratio);
    angle = if steep { FRAC_PI_2 - angle } else { angle };
    angle = if x < 0.0 { PI - angle } else { angle };
    angle = if ax + ay == 0.0 { 0.0 } else { angle };
    angle.copysign(y)
}

#[inline]
#[must_use]
pub fn fast_arg(z: Complex<f32>) -> f32 {
    fast_atan2(z.im, z.re)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f64 = 1e-6;

    fn angle_error(fast: f32, exact: f64) -> f64 {
        let diff = (f64::from(fast) - exact).rem_euclid(std::f64::consts::TAU);
        diff.min(std::f64::consts::TAU - diff)
    }

    #[test]
    fn matches_atan2_over_a_dense_grid_of_angles_and_magnitudes() {
        let magnitudes = [
            1e-44, 1e-40, 1e-38, 1e-30, 1e-10, 1e-3, 0.5, 1.0, 7.0, 1e5, 1e20, 1e35, 3e38,
        ];
        let mut worst = 0.0f64;
        for &magnitude in &magnitudes {
            for k in 0..20_000 {
                let theta = -std::f64::consts::PI + std::f64::consts::TAU * f64::from(k) / 20_000.0;
                let (y, x) = (
                    (magnitude * theta.sin()) as f32,
                    (magnitude * theta.cos()) as f32,
                );
                if x == 0.0 && y == 0.0 {
                    continue;
                }
                let exact = f64::from(y).atan2(f64::from(x));
                worst = worst.max(angle_error(fast_atan2(y, x), exact));
            }
        }
        assert!(worst < TOLERANCE, "worst error {worst}");
    }

    #[test]
    fn matches_atan2_over_a_ratio_sweep_in_every_quadrant() {
        let mut worst = 0.0f64;
        for k in 0..=100_000 {
            let ratio = k as f32 / 100_000.0;
            for (y, x) in [
                (ratio, 1.0),
                (1.0, ratio),
                (-ratio, 1.0),
                (ratio, -1.0),
                (-1.0, -ratio),
                (1.0, -ratio),
            ] {
                let exact = f64::from(y).atan2(f64::from(x));
                worst = worst.max(angle_error(fast_atan2(y, x), exact));
            }
        }
        assert!(worst < TOLERANCE, "worst error {worst}");
    }

    #[test]
    fn zero_over_zero_is_zero() {
        for (y, x) in [(0.0f32, 0.0f32), (-0.0, 0.0), (0.0, -0.0), (-0.0, -0.0)] {
            let angle = fast_atan2(y, x);
            assert_eq!(angle, 0.0, "atan2({y}, {x})");
            assert_eq!(angle.is_sign_negative(), y.is_sign_negative());
        }
        assert_eq!(fast_arg(Complex::new(0.0, 0.0)), 0.0);
    }

    #[test]
    fn signed_zeros_and_axes_follow_atan2() {
        let cases = [
            (0.0f32, 1.0f32),
            (-0.0, 1.0),
            (0.0, -1.0),
            (-0.0, -1.0),
            (1.0, 0.0),
            (-1.0, 0.0),
            (1.0, -0.0),
            (-1.0, -0.0),
            (1.0, 1.0),
            (-1.0, -1.0),
            (1.0, -1.0),
            (-1.0, 1.0),
        ];
        for (y, x) in cases {
            let fast = fast_atan2(y, x);
            let exact = y.atan2(x);
            assert!((fast - exact).abs() < 1e-6, "atan2({y}, {x}) = {fast}");
            assert_eq!(fast.is_sign_negative(), exact.is_sign_negative());
        }
    }

    #[test]
    fn infinities_follow_atan2() {
        let inf = f32::INFINITY;
        for (y, x) in [
            (inf, inf),
            (inf, -inf),
            (-inf, inf),
            (-inf, -inf),
            (inf, 1.0),
            (-inf, -1.0),
            (1.0, inf),
            (1.0, -inf),
            (-1.0, -inf),
            (0.0, -inf),
        ] {
            let fast = fast_atan2(y, x);
            let exact = y.atan2(x);
            assert!((fast - exact).abs() < 1e-6, "atan2({y}, {x}) = {fast}");
        }
    }

    #[test]
    fn nan_propagates() {
        for (y, x) in [
            (f32::NAN, 1.0),
            (1.0, f32::NAN),
            (f32::NAN, f32::NAN),
            (f32::NAN, 0.0),
            (0.0, f32::NAN),
            (f32::NAN, f32::INFINITY),
        ] {
            assert!(fast_atan2(y, x).is_nan(), "atan2({y}, {x})");
        }
    }
}

use super::MANTISSA_BITS;

const EXP2_SERIES: [f32; 7] = [
    1.0,
    std::f32::consts::LN_2,
    0.240_226_5,
    0.055_504_11,
    0.009_618_129,
    0.001_333_356,
    0.000_154_035_3,
];
const EXP2_MIN: f32 = -126.0;
const EXP2_MAX: f32 = 127.499_99;
/// The bias of a single-precision exponent field.
const EXPONENT_BIAS: i32 = 127;
const POWER_OCTAVES_PER_DB: f32 = 0.1 * std::f32::consts::LOG2_10;

#[must_use]
#[inline(always)]
pub fn fast_exp2(x: f32) -> f32 {
    if x.is_nan() {
        return x;
    }
    if x < EXP2_MIN {
        return 0.0;
    }
    if x > EXP2_MAX {
        return f32::INFINITY;
    }
    let whole = x.round();
    let t = x - whole;
    let [c0, c1, c2, c3, c4, c5, c6] = EXP2_SERIES;
    let fraction = c0 + t * (c1 + t * (c2 + t * (c3 + t * (c4 + t * (c5 + t * c6)))));
    let scale = f32::from_bits(((whole as i32 + EXPONENT_BIAS) as u32) << MANTISSA_BITS);
    fraction * scale
}

#[must_use]
#[inline(always)]
pub fn fast_db_to_power(db: f32) -> f32 {
    fast_exp2(db * POWER_OCTAVES_PER_DB)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_to_power_tracks_powf_across_the_display_range() {
        let worst = (-2_000..=2_000)
            .map(|step| step as f32 * 0.1)
            .map(|db| {
                let exact = 10f64.powf(f64::from(db) / 10.0);
                (f64::from(fast_db_to_power(db)) / exact - 1.0).abs()
            })
            .fold(0.0f64, f64::max);
        assert!(worst < 5e-6, "worst relative error {worst}");
    }

    #[test]
    fn exp2_edges_follow_std() {
        assert_eq!(fast_exp2(0.0), 1.0);
        assert_eq!(fast_exp2(10.0), 1024.0);
        assert_eq!(fast_exp2(-1.0), 0.5);
        assert_eq!(fast_exp2(f32::NEG_INFINITY), 0.0);
        assert_eq!(fast_exp2(-500.0), 0.0);
        assert_eq!(fast_exp2(f32::INFINITY), f32::INFINITY);
        assert!(fast_exp2(f32::NAN).is_nan());
        assert_eq!(fast_db_to_power(f32::NEG_INFINITY), 0.0);
    }
}

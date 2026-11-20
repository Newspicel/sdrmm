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
}

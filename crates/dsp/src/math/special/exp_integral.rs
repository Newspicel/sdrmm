use std::f64::consts::EULER_GAMMA;

/// Series coefficients of E1 below one (Abramowitz and Stegun 5.1.53).
const SERIES: [f64; 5] = [
    0.999_991_93,
    -0.249_910_55,
    0.055_199_68,
    -0.009_760_04,
    0.001_078_57,
];
/// Rational fit of x e^x E1(x) from one up (Abramowitz and Stegun 5.1.56).
const NUMERATOR: [f64; 4] = [
    8.573_328_740_1,
    18.059_016_973,
    8.634_760_892_5,
    0.267_773_734_3,
];
const DENOMINATOR: [f64; 4] = [
    9.573_322_345_4,
    25.632_956_148_6,
    21.099_653_082_7,
    3.958_496_922_8,
];

/// The exponential integral E1(x) for positive `x`.
#[must_use]
pub fn exponential_integral(x: f32) -> f32 {
    let x = f64::from(x);
    let value = if x < 1.0 {
        -x.ln() - EULER_GAMMA
            + x * (SERIES[0] + x * (SERIES[1] + x * (SERIES[2] + x * (SERIES[3] + x * SERIES[4]))))
    } else {
        let numerator = monic_quartic(x, &NUMERATOR);
        let denominator = monic_quartic(x, &DENOMINATOR);
        (-x).exp() / x * (numerator / denominator)
    };
    value as f32
}

/// `x^4 + c0 x^3 + c1 x^2 + c2 x + c3`, by Horner.
fn monic_quartic(x: f64, c: &[f64; 4]) -> f64 {
    x * (x * (x * (x + c[0]) + c[1]) + c[2]) + c[3]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_integral_matches_its_reference_values() {
        let reference = [
            (0.01f32, 4.037_93f32),
            (0.1, 1.822_924),
            (0.5, 0.559_774),
            (1.0, 0.219_384),
            (2.0, 0.048_900_51),
            (5.0, 0.001_148_296),
            (10.0, 4.156_969e-6),
        ];
        for (x, expected) in reference {
            let error = (exponential_integral(x) - expected).abs() / expected;
            assert!(
                error < 1e-4,
                "E1({x}) = {} not {expected}",
                exponential_integral(x)
            );
        }
    }
}

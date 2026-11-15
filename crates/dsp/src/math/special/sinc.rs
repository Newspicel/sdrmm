use std::f64::consts::PI;

const ZERO_ARGUMENT: f64 = 1e-12;

/// Normalised sinc, `sin(pi x) / (pi x)`.
#[must_use]
pub fn sinc(x: f64) -> f64 {
    if x.abs() < ZERO_ARGUMENT {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sinc_is_one_at_zero_and_zero_at_integers() {
        assert_eq!(sinc(0.0), 1.0);
        assert!(sinc(1.0).abs() < 1e-15);
        assert!(sinc(-3.0).abs() < 1e-15);
        assert!((sinc(0.5) - 2.0 / PI).abs() < 1e-15);
    }
}

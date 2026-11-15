use num_complex::Complex;

#[must_use]
pub fn widen(value: Complex<f32>) -> Complex<f64> {
    Complex::new(f64::from(value.re), f64::from(value.im))
}

#[must_use]
pub fn narrow(value: Complex<f64>) -> Complex<f32> {
    Complex::new(value.re as f32, value.im as f32)
}

/// Mean power of a block, accumulated in `f64` so long blocks keep their precision.
#[must_use]
pub fn mean_power(samples: &[Complex<f32>]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|v| f64::from(v.norm_sqr())).sum();
    (sum / samples.len() as f64) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widen_and_narrow_round_trip() {
        let value = Complex::new(1.5f32, -0.25);
        assert_eq!(narrow(widen(value)), value);
    }

    #[test]
    fn mean_power_of_nothing_is_zero() {
        assert_eq!(mean_power(&[]), 0.0);
        assert_eq!(mean_power(&[Complex::new(3.0, 4.0)]), 25.0);
    }
}

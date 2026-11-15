use num_complex::Complex;

use crate::math::db::Decibels;

pub const EQ_POINTS: usize = 64;

const EQ_LIMIT_DB: f32 = 6.0;

#[must_use]
pub fn equaliser_frequency(point: usize, points: usize) -> f32 {
    -0.5 + point as f32 / points.max(1) as f32
}

#[must_use]
pub fn equaliser_at(equaliser: &[Complex<f32>], frequency: f32) -> Complex<f32> {
    let points = equaliser.len();
    if points == 0 {
        return Complex::new(1.0, 0.0);
    }
    let position = (f64::from(frequency) + 0.5).rem_euclid(1.0) * points as f64;
    let below = position.floor();
    let fraction = (position - below) as f32;
    let below = below as usize % points;
    let above = (below + 1) % points;
    equaliser[below] * (1.0 - fraction) + equaliser[above] * fraction
}

/// Limits an equaliser point to `EQ_LIMIT_DB` either way, keeping its phase.
pub(super) fn clip(value: Complex<f32>) -> Complex<f32> {
    let low = (-EQ_LIMIT_DB).db_to_amplitude();
    let high = EQ_LIMIT_DB.db_to_amplitude();
    let magnitude = value.norm();
    if !magnitude.is_finite() || magnitude <= f32::MIN_POSITIVE {
        return Complex::new(low, 0.0);
    }
    value * (magnitude.clamp(low, high) / magnitude)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equaliser_points_run_from_minus_half_to_half() {
        let equaliser: Vec<Complex<f32>> = (0..4)
            .map(|point| Complex::new(point as f32, 0.0))
            .collect();
        assert!((equaliser_frequency(0, 4) + 0.5).abs() < f32::EPSILON);
        assert!(equaliser_frequency(2, 4).abs() < f32::EPSILON);
        assert!((equaliser_at(&equaliser, 0.0).re - 2.0).abs() < 1e-6);
        assert!((equaliser_at(&equaliser, 0.125).re - 2.5).abs() < 1e-6);
        assert!((equaliser_at(&equaliser, 0.375).re - 1.5).abs() < 1e-6);
        assert!((equaliser_at(&[], 0.1) - Complex::new(1.0, 0.0)).norm() < f32::EPSILON);
    }
}

use crate::math::special::{norm_deg, wrap_deg};

const QUARTER_TURN_DEG: f64 = 90.0;

/// The bearing a line array cannot tell from `azimuth_deg`: its reflection about the axis.
#[must_use]
pub fn mirror_deg(azimuth_deg: f64, axis_deg: f64) -> f64 {
    norm_deg(2.0 * axis_deg - azimuth_deg)
}

#[must_use]
pub fn on_side(azimuth_deg: f64, centre_deg: f64) -> bool {
    wrap_deg(azimuth_deg - centre_deg).abs() <= QUARTER_TURN_DEG
}

#[must_use]
pub fn apart_deg(a: f64, b: f64) -> f64 {
    wrap_deg(a - b).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_and_side_follow_the_axis() {
        assert!((mirror_deg(30.0, 90.0) - 150.0).abs() < 1e-12);
        assert!((mirror_deg(60.0, 180.0) - 300.0).abs() < 1e-12);
        assert!(on_side(30.0, 0.0));
        assert!(on_side(270.0, 0.0));
        assert!(!on_side(150.0, 0.0));
        assert!((apart_deg(359.0, 1.0) - 2.0).abs() < 1e-12);
    }
}

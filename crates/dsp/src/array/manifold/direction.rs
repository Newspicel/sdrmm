use super::Vec3;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Direction {
    pub azimuth_deg: f64,
    pub elevation_deg: f64,
}

impl Direction {
    #[must_use]
    pub const fn new(azimuth_deg: f64, elevation_deg: f64) -> Self {
        Self {
            azimuth_deg,
            elevation_deg,
        }
    }

    #[must_use]
    pub const fn horizon(azimuth_deg: f64) -> Self {
        Self::new(azimuth_deg, 0.0)
    }

    /// Unit vector toward the direction: x east, y north, z up, azimuth clockwise from north.
    #[must_use]
    pub fn unit(&self) -> Vec3 {
        let (sin_az, cos_az) = self.azimuth_deg.to_radians().sin_cos();
        let (sin_el, cos_el) = self.elevation_deg.to_radians().sin_cos();
        Vec3::new(sin_az * cos_el, cos_az * cos_el, sin_el)
    }

    #[must_use]
    pub(super) fn offset(self, by: Self) -> Self {
        Self::new(
            self.azimuth_deg + by.azimuth_deg,
            self.elevation_deg + by.elevation_deg,
        )
    }
}

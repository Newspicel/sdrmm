#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    #[must_use]
    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    #[must_use]
    pub fn plus(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    #[must_use]
    pub fn minus(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    #[must_use]
    pub fn scaled(self, factor: f64) -> Self {
        Self::new(self.x * factor, self.y * factor, self.z * factor)
    }

    #[must_use]
    pub fn norm(self) -> f64 {
        self.dot(self).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_arithmetic_works_per_axis() {
        let a = Vec3::new(1.0, 2.0, 2.0);
        let b = Vec3::new(0.5, -1.0, 4.0);
        assert_eq!(a.plus(b), Vec3::new(1.5, 1.0, 6.0));
        assert_eq!(a.minus(b), Vec3::new(0.5, 3.0, -2.0));
        assert_eq!(a.scaled(2.0), Vec3::new(2.0, 4.0, 4.0));
        assert_eq!(a.dot(b), 6.5);
        assert_eq!(a.norm(), 3.0);
    }
}

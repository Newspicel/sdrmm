use std::ops::{Add, AddAssign, Sub, SubAssign};

/// Assignment cost ordered first by how many forbidden pairs it holds, then by its total, so
/// the solver never trades a forbidden pair for a cheaper sum.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub(super) struct Cost {
    forbidden: f64,
    total: f64,
}

impl Cost {
    pub(super) const OPEN: Self = Self {
        forbidden: 0.0,
        total: 0.0,
    };
    pub(super) const FORBIDDEN: Self = Self {
        forbidden: 1.0,
        total: 0.0,
    };
    pub(super) const UNBOUNDED: Self = Self {
        forbidden: f64::INFINITY,
        total: f64::INFINITY,
    };

    pub(super) fn of(value: Option<f64>) -> Self {
        match value {
            Some(total) if total.is_finite() => Self {
                forbidden: 0.0,
                total,
            },
            _ => Self::FORBIDDEN,
        }
    }

    pub(super) fn allowed(self) -> bool {
        self.forbidden == 0.0
    }
}

impl Add for Cost {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            forbidden: self.forbidden + other.forbidden,
            total: self.total + other.total,
        }
    }
}

impl Sub for Cost {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            forbidden: self.forbidden - other.forbidden,
            total: self.total - other.total,
        }
    }
}

impl AddAssign for Cost {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl SubAssign for Cost {
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

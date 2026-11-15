use crate::array::beamform::{BeamError, adaptive::check_step};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Adaptation {
    Nlms { step: f32 },
    Rls { forget: f32 },
}

impl Adaptation {
    pub(super) fn check(self) -> Result<(), BeamError> {
        match self {
            Self::Nlms { step } => check_step(step),
            Self::Rls { forget } if forget.is_finite() && forget > 0.0 && forget <= 1.0 => Ok(()),
            Self::Rls { .. } => Err(BeamError::Setting("forget")),
        }
    }

    pub(super) const fn is_rls(self) -> bool {
        matches!(self, Self::Rls { .. })
    }
}

#[must_use]
pub fn tdl_cmac_per_sample(references: usize, taps: usize, adaptation: Adaptation) -> f64 {
    let m = (references * taps) as f64;
    match adaptation {
        Adaptation::Nlms { .. } => 2.0 * m + 2.0,
        Adaptation::Rls { .. } => 3.0 * m * m + 4.0 * m,
    }
}

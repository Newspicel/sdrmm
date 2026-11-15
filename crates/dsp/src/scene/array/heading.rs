#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HeadingTrack {
    Fixed(f64),
    Rotating { start_deg: f64, rate_dps: f64 },
}

impl HeadingTrack {
    #[must_use]
    pub fn at(self, t_s: f64) -> f64 {
        match self {
            Self::Fixed(heading) => heading,
            Self::Rotating {
                start_deg,
                rate_dps,
            } => start_deg + rate_dps * t_s,
        }
    }

    pub(super) const fn is_finite(self) -> bool {
        match self {
            Self::Fixed(heading) => heading.is_finite(),
            Self::Rotating {
                start_deg,
                rate_dps,
            } => start_deg.is_finite() && rate_dps.is_finite(),
        }
    }
}

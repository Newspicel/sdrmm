use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ControlVerb {
    Tune,
    Doppler,
    Sweep,
    Point,
}

impl ControlVerb {
    #[must_use]
    pub const fn noun(self) -> &'static str {
        match self {
            Self::Tune => "tuning",
            Self::Doppler => "Doppler",
            Self::Sweep => "sweeps",
            Self::Point => "pointing",
        }
    }
}

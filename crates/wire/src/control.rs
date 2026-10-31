use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    HuntAction, HuntStatus, HuntSweepParams, PassPoint, SatelliteLook, SatellitePass, ScanAction,
    ScanSettings, ScannerStatus,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ControlVerb {
    Tune,
    Doppler,
    Sweep,
    Point,
    Run,
}

impl ControlVerb {
    #[must_use]
    pub const fn noun(self) -> &'static str {
        match self {
            Self::Tune => "tuning",
            Self::Doppler => "Doppler",
            Self::Sweep => "sweeps",
            Self::Point => "pointing",
            Self::Run => "running",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DopplerSteer {
    pub shift_hz: f64,
    pub rate_hz_s: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "job", rename_all = "snake_case")]
pub enum SweepJob {
    Scan {
        settings: ScanSettings,
        action: ScanAction,
    },
    Hunt {
        params: HuntSweepParams,
        action: HuntAction,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SkyTrack {
    pub look: SatelliteLook,
    pub visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_pass: Option<SatellitePass>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pass_track: Vec<PassPoint>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "command", content = "with", rename_all = "snake_case")]
pub enum ControlCommand {
    Tune {
        freq_hz: f64,
    },
    Doppler(DopplerSteer),
    Sweep(SweepJob),
    Point(SkyTrack),
    /// Holds a radio or a recorder on, or lets it rest. One wired controller saying on is enough.
    Run {
        on: bool,
    },
    Release,
}

impl ControlCommand {
    #[must_use]
    pub const fn verb(&self) -> Option<ControlVerb> {
        match self {
            Self::Tune { .. } => Some(ControlVerb::Tune),
            Self::Doppler(_) => Some(ControlVerb::Doppler),
            Self::Sweep(_) => Some(ControlVerb::Sweep),
            Self::Point(_) => Some(ControlVerb::Point),
            Self::Run { .. } => Some(ControlVerb::Run),
            Self::Release => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "job", content = "status", rename_all = "snake_case")]
pub enum SweepStatus {
    Scan(ScannerStatus),
    Hunt(HuntStatus),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ControlAck {
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verb: Option<ControlVerb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep: Option<Box<SweepStatus>>,
}

#[cfg(test)]
mod tests;

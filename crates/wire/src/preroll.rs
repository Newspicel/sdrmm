use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::state::RecordingStatus;

pub const MIN_PRE_ROLL_S: u32 = 1;
pub const MAX_PRE_ROLL_S: u32 = 120;
pub const DEFAULT_PRE_ROLL_S: u32 = 10;
pub const MAX_PRE_ROLL_BYTES: u64 = 1 << 30;

/// What a recorder holds in memory ahead of the record button, and what it has written since the
/// button went down.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PreRollStatus {
    pub node: String,
    pub stream: u32,
    pub pre_roll_s: u32,
    pub sample_rate: u64,
    pub center_hz: i64,
    pub held_samples: u64,
    pub capacity_samples: u64,
    pub overruns: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<RecordingStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl PreRollStatus {
    #[must_use]
    pub fn held_seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.held_samples as f64 / self.sample_rate as f64
        }
    }
}

#[must_use]
pub const fn pre_roll_valid(seconds: u32) -> bool {
    seconds == 0 || (MIN_PRE_ROLL_S <= seconds && seconds <= MAX_PRE_ROLL_S)
}

#[must_use]
pub fn pre_roll_capacity_samples(seconds: u32, sample_rate: f64) -> u64 {
    (f64::from(seconds) * sample_rate).ceil().max(0.0) as u64
}

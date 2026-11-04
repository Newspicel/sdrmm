use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::choices::ParamChoice;
use crate::present::Field;

/// What a running decoder reports about itself between events: the standard health footer,
/// readouts for its node face and the options it found for `Live` settings, keyed by the
/// setting.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ChannelStatus {
    #[serde(default, skip_serializing_if = "ChannelHealth::is_empty")]
    pub health: ChannelHealth,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readouts: Vec<Field>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<ParamChoice>,
}

impl ChannelStatus {
    #[must_use]
    pub fn readouts(readouts: Vec<Field>) -> Self {
        Self {
            readouts,
            ..Self::default()
        }
    }

    /// The text of the readout keyed `key`, as a `Choice` option's `status` names it.
    #[must_use]
    pub fn readout(&self, key: &str) -> Option<&Field> {
        self.readouts.iter().find(|field| field.key == key)
    }
}

/// The few facts every channel face shows in one footer, whatever the decoder. Each is unset
/// while the decoder cannot tell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ChannelHealth {
    /// The decoder holds sync, carrier or frame lock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snr_db: Option<f32>,
    /// How far the signal sits from the tuned frequency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freq_error_hz: Option<f64>,
    /// Frames or blocks that failed their check, per second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errors_per_s: Option<f32>,
}

impl ChannelHealth {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    #[must_use]
    pub fn lock(self, locked: bool) -> Self {
        Self {
            lock: Some(locked),
            ..self
        }
    }

    #[must_use]
    pub fn snr(self, snr_db: f32) -> Self {
        Self {
            snr_db: Some(snr_db).filter(|db| db.is_finite()),
            ..self
        }
    }

    #[must_use]
    pub fn freq_error(self, hz: f64) -> Self {
        Self {
            freq_error_hz: Some(hz).filter(|hz| hz.is_finite()),
            ..self
        }
    }

    #[must_use]
    pub fn errors(self, per_s: f32) -> Self {
        Self {
            errors_per_s: Some(per_s).filter(|rate| rate.is_finite()),
            ..self
        }
    }
}

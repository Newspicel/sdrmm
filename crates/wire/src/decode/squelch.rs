use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{EventContract, EventKind};

/// The subaudible tone or code a receiver hears under the audio, and whether it lets audio out.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ToneSquelchStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctcss_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dcs_code: Option<u16>,
    pub open: bool,
}

impl EventContract for ToneSquelchStatus {
    const KIND: EventKind = EventKind::named("tone");
}

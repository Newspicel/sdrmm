use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct WsjtMessage {
    pub text: String,
    pub snr_db: f32,
    pub audio_hz: f32,
    pub time_offset_s: f32,
    pub hard_errors: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct WsprSpot {
    pub text: String,
    pub callsign: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<String>,
    pub power_dbm: i32,
    pub snr_db: f32,
    pub audio_hz: f32,
    pub time_offset_s: f32,
    pub drift_hz: f32,
}

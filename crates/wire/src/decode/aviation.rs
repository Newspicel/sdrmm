use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AdsbMessage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_12mhz: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_level: Option<u8>,
    pub icao: String,
    pub df: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_code: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callsign: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub altitude_ft: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground_speed_kt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_deg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_rate_fpm: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub squawk: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_ground: Option<bool>,
    pub raw: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AcarsMessage {
    pub mode: char,
    pub registration: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ack: Option<char>,
    pub label: String,
    pub block_id: char,
    pub downlink: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq_no: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flight: Option<String>,
    pub text: String,
    pub more: bool,
}

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct GnssFrame {
    pub prn: u8,
    pub doppler_hz: f32,
    pub code_phase_chips: f32,
    pub cn0_db_hz: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subframe: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tow_seconds: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub week: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VorReading {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station_lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station_lon: Option<f64>,
    pub magnetic_declination_deg: f64,
    pub radial_deg: f64,
    pub variable_phase_deg: f64,
    pub reference_phase_deg: f64,
    pub signal_db: f32,
    pub confidence: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IlsReading {
    pub component: crate::channel::IlsComponent,
    pub modulation_90: f32,
    pub modulation_150: f32,
    pub ddm: f32,
    pub deviation_dots: f32,
    pub signal_db: f32,
}

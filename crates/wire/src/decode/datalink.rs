use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EotBattery {
    NotMonitored,
    VeryLow,
    Low,
    Ok,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EotArming {
    Normal,
    Arming,
    Armed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HotCommand {
    StatusRequest,
    Emergency,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct EotStatus {
    pub message_type: u8,
    pub arming: EotArming,
    pub pressure_psig: u8,
    pub battery: EotBattery,
    pub battery_charge_pct: u8,
    pub valve_ok: bool,
    pub confirmed: bool,
    pub turbine: bool,
    pub motion: bool,
    pub marker_light: bool,
    pub marker_battery_low: bool,
    pub discretionary: bool,
    pub chaining: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct HotRequest {
    pub command: HotCommand,
    pub code: u8,
    pub copies: u8,
}

impl HotRequest {
    #[must_use]
    pub fn command_label(&self) -> String {
        match self.command {
            HotCommand::StatusRequest => "status request".to_owned(),
            HotCommand::Emergency => "EMERGENCY".to_owned(),
            HotCommand::Other => format!("command {:#04x}", self.code),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "unit", rename_all = "snake_case")]
pub enum EotReport {
    Rear(EotStatus),
    Head(HotRequest),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct EotMessage {
    pub unit_address: u32,
    pub report: EotReport,
    pub errors_corrected: u32,
}

impl EotMessage {
    #[must_use]
    pub fn summary(&self) -> String {
        match &self.report {
            EotReport::Rear(s) => {
                let mut parts = vec![
                    format!("EOT {}", self.unit_address),
                    format!("{} psig", s.pressure_psig),
                    if s.motion { "moving" } else { "stopped" }.to_owned(),
                ];
                if s.arming != EotArming::Normal {
                    parts.push(format!("{:?}", s.arming).to_lowercase());
                }
                parts.join(" · ")
            }
            EotReport::Head(h) => format!("HOT {} · {}", self.unit_address, h.command_label()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AprsPacket {
    pub source: String,
    pub destination: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
    pub info: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub course_deg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_kt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub altitude_ft: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mic_e_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weather: Option<crate::weather::AprsWeather>,
    pub tnc2: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DataLinkMessage {
    pub message_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub crc_ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fec_corrected: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snr_db: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_error_hz: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    pub details: serde_json::Value,
}

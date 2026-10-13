use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AisMessage {
    pub mmsi: u32,
    pub msg_type: u8,
    pub ais_channel: char,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_sign: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sog_kt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cog_deg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_deg: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nav_status: Option<u8>,
    pub nmea: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NavtexMessage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<char>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<char>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<u8>,
    pub text: String,
    pub errors_corrected: u32,
    pub complete: bool,
}

impl NavtexMessage {
    #[must_use]
    pub fn header(&self) -> Option<String> {
        let (station, subject, serial) = (self.station?, self.subject?, self.serial?);
        Some(format!("{station}{subject}{serial:02}"))
    }

    #[must_use]
    pub fn subject_name(subject: char) -> Option<&'static str> {
        Some(match subject.to_ascii_uppercase() {
            'A' => "Navigational warning",
            'B' => "Meteorological warning",
            'C' => "Ice report",
            'D' => "Search and rescue / piracy",
            'E' => "Meteorological forecast",
            'F' => "Pilot service",
            'G' => "AIS",
            'H' => "LORAN",
            'J' => "SATNAV",
            'K' => "Other electronic navaid",
            'L' => "Navigational warning (additional)",
            'T' => "Test transmission",
            'V' => "Notice to fishermen",
            'W' => "Environmental",
            'X' | 'Y' => "Special service",
            'Z' => "No message on hand",
            _ => return None,
        })
    }
}

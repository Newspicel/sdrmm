use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub const SARSAT_BANDWIDTH_HZ: f64 = 20_000.0;
pub const MIN_SARSAT_BANDWIDTH_HZ: f64 = 4_000.0;
pub const MAX_SARSAT_BANDWIDTH_HZ: f64 = 22_000.0;

fn default_sarsat_bandwidth_hz() -> f64 {
    SARSAT_BANDWIDTH_HZ
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SarsatParams {
    #[serde(default = "default_sarsat_bandwidth_hz")]
    pub bandwidth_hz: f64,
}

impl Default for SarsatParams {
    fn default() -> Self {
        Self {
            bandwidth_hz: default_sarsat_bandwidth_hz(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SarsatBeacon {
    Epirb,
    Elt,
    Plb,
    ShipSecurity,
    Test,
    Other,
}

impl SarsatBeacon {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Epirb => "EPIRB",
            Self::Elt => "ELT",
            Self::Plb => "PLB",
            Self::ShipSecurity => "SSAS",
            Self::Test => "Test",
            Self::Other => "Beacon",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SarsatProtocol {
    MaritimeUser,
    RadioCallSignUser,
    AviationUser,
    SerialUser,
    NationalUser,
    TestUser,
    Orbitography,
    SecondGeneration,
    StandardLocation,
    ShipSecurity,
    NationalLocation,
    RlsLocation,
    EltDtLocation,
    StandardTest,
    NationalTest,
    Spare,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SarsatMessage {
    pub hex_id: String,
    pub message: String,
    pub long: bool,
    pub self_test: bool,
    pub country: u16,
    pub protocol: SarsatProtocol,
    pub beacon: SarsatBeacon,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homing: Option<bool>,
    pub errors_corrected: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf2_ok: Option<bool>,
}

impl SarsatMessage {
    #[must_use]
    pub fn position(&self) -> Option<(f64, f64)> {
        self.lat.zip(self.lon)
    }

    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = vec![format!("{} {}", self.beacon.label(), self.hex_id)];
        parts.extend(self.identity.clone());
        if let Some((lat, lon)) = self.position() {
            parts.push(format!("{lat:.4}, {lon:.4}"));
        }
        if self.self_test {
            parts.push("self-test".to_owned());
        }
        if self.pdf2_ok == Some(false) {
            parts.push("PDF-2 failed".to_owned());
        }
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message() -> SarsatMessage {
        SarsatMessage {
            hex_id: "ADCD00800440401".to_owned(),
            message: "56E6804002202009655250".to_owned(),
            long: false,
            self_test: false,
            country: 366,
            protocol: SarsatProtocol::SerialUser,
            beacon: SarsatBeacon::Epirb,
            identity: Some("S/N 1".to_owned()),
            lat: None,
            lon: None,
            homing: Some(true),
            errors_corrected: 0,
            pdf2_ok: None,
        }
    }

    #[test]
    fn a_beacon_without_a_fix_omits_its_position() {
        let json = serde_json::to_value(message()).unwrap();
        assert!(json.get("lat").is_none());
        assert_eq!(json["protocol"], "serial_user");
        assert_eq!(json["beacon"], "epirb");
        assert_eq!(message().summary(), "EPIRB ADCD00800440401 · S/N 1");
        let back: SarsatMessage = serde_json::from_value(json).unwrap();
        assert_eq!(back, message());
    }

    #[test]
    fn the_summary_flags_a_test_and_a_failed_second_field() {
        let flagged = SarsatMessage {
            lat: Some(43.5333),
            lon: Some(-1.5),
            self_test: true,
            pdf2_ok: Some(false),
            identity: None,
            ..message()
        };
        assert_eq!(flagged.position(), Some((43.5333, -1.5)));
        assert_eq!(
            flagged.summary(),
            "EPIRB ADCD00800440401 · 43.5333, -1.5000 · self-test · PDF-2 failed"
        );
    }
}

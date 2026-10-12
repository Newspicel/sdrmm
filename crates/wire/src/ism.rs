use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub const ISM_SAMPLE_RATE_HZ: f64 = 250_000.0;
pub const ISM_BANDWIDTH_HZ: f64 = 240_000.0;

fn default_true() -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct IsmParams {
    #[serde(default = "default_true")]
    pub unknown: bool,
}

impl Default for IsmParams {
    fn default() -> Self {
        Self { unknown: true }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IsmModulation {
    #[default]
    Ook,
    Fsk,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IsmCoding {
    #[default]
    Pwm,
    Ppm,
    Pcm,
    Manchester,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IsmIntegrity {
    Crc,
    Checksum,
    #[default]
    Repeats,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IsmMessage {
    pub model: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery_ok: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature_c: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature_2_c: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub humidity_pct: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_kpa: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wind_avg_m_s: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wind_max_m_s: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wind_dir_deg: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rain_mm: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv_index: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light_lux: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_open: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tamper: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alarm: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tristate: Option<String>,
    pub modulation: IsmModulation,
    pub coding: IsmCoding,
    pub integrity: IsmIntegrity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_us: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_us: Option<u16>,
    pub bits: u16,
    pub raw: String,
    pub snr_db: f32,
}

impl IsmMessage {
    #[must_use]
    pub fn station(&self) -> String {
        format!("{} {}", self.model, self.id)
    }

    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = vec![self.station()];
        parts.extend(self.channel.as_ref().map(|channel| format!("ch {channel}")));
        parts.extend(self.readings());
        if self.battery_ok == Some(false) {
            parts.push("battery low".to_owned());
        }
        parts.join(" · ")
    }

    fn readings(&self) -> Vec<String> {
        let mut parts = Vec::new();
        parts.extend(self.temperature_c.map(|t| format!("{t:.1} °C")));
        parts.extend(self.temperature_2_c.map(|t| format!("{t:.1} °C")));
        parts.extend(self.humidity_pct.map(|h| format!("{h}%")));
        parts.extend(self.pressure_kpa.map(|p| format!("{p:.0} kPa")));
        parts.extend(self.wind_avg_m_s.map(|w| format!("wind {w:.1} m/s")));
        parts.extend(self.wind_max_m_s.map(|w| format!("gust {w:.1} m/s")));
        parts.extend(self.wind_dir_deg.map(|d| format!("{d:.0}°")));
        parts.extend(self.rain_mm.map(|r| format!("rain {r:.1} mm")));
        parts.extend(self.uv_index.map(|uv| format!("UV {uv:.1}")));
        parts.extend(self.light_lux.map(|lux| format!("{lux:.0} lx")));
        parts.extend(
            self.contact_open
                .map(|open| if open { "open" } else { "closed" }.to_owned()),
        );
        if self.tamper == Some(true) {
            parts.push("tamper".to_owned());
        }
        if self.alarm == Some(true) {
            parts.push("alarm".to_owned());
        }
        parts.extend(self.command.map(|command| format!("cmd {command}")));
        if self.model == UNKNOWN_MODEL {
            parts.push(format!("{} bits", self.bits));
        }
        parts
    }
}

pub const UNKNOWN_MODEL: &str = "Unknown";

#[cfg(test)]
mod tests {
    use super::*;

    fn tower() -> IsmMessage {
        IsmMessage {
            model: "Acurite-Tower".to_owned(),
            id: "12053".to_owned(),
            channel: Some("C".to_owned()),
            battery_ok: Some(true),
            temperature_c: Some(26.7),
            humidity_pct: Some(74),
            integrity: IsmIntegrity::Checksum,
            bits: 56,
            raw: "2f1544ca12d4a6".to_owned(),
            snr_db: 21.5,
            ..IsmMessage::default()
        }
    }

    #[test]
    fn a_sensor_reads_as_its_model_id_and_readings() {
        assert_eq!(
            tower().summary(),
            "Acurite-Tower 12053 · ch C · 26.7 °C · 74%"
        );
        let low = IsmMessage {
            battery_ok: Some(false),
            ..tower()
        };
        assert!(low.summary().ends_with("battery low"));
    }

    #[test]
    fn absent_readings_stay_off_the_wire() {
        let json = serde_json::to_value(tower()).unwrap();
        assert!(json.get("wind_avg_m_s").is_none());
        assert_eq!(json["integrity"], "checksum");
        assert_eq!(json["modulation"], "ook");
        let back: IsmMessage = serde_json::from_value(json).unwrap();
        assert_eq!(back, tower());
    }

    #[test]
    fn an_unknown_code_names_its_length() {
        let unknown = IsmMessage {
            model: UNKNOWN_MODEL.to_owned(),
            id: "a5f0c3".to_owned(),
            bits: 24,
            ..IsmMessage::default()
        };
        assert_eq!(unknown.summary(), "Unknown a5f0c3 · 24 bits");
    }

    #[test]
    fn params_default_to_showing_unknown_codes() {
        let params: IsmParams = serde_json::from_str("{}").unwrap();
        assert!(params.unknown);
    }
}

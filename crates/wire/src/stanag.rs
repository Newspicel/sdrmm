use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub const STANAG_WINDOW_S: u8 = 2;
pub const MIN_STANAG_WINDOW_S: u8 = 2;
pub const MAX_STANAG_WINDOW_S: u8 = 12;

fn default_stanag_window_s() -> u8 {
    STANAG_WINDOW_S
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct StanagParams {
    #[serde(default = "default_stanag_window_s")]
    pub window_s: u8,
}

impl Default for StanagParams {
    fn default() -> Self {
        Self {
            window_s: default_stanag_window_s(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StanagModem {
    #[serde(rename = "stanag_4285")]
    Stanag4285,
    #[serde(rename = "stanag_4529")]
    Stanag4529,
    #[serde(rename = "mil_std_188_110a")]
    MilStd110a,
    #[serde(rename = "stanag_4539")]
    Stanag4539,
    ThirtyNineTone,
    SixteenTone,
    #[serde(rename = "ale_2g")]
    Ale2g,
}

impl StanagModem {
    pub const ALL: [Self; 7] = [
        Self::Stanag4285,
        Self::Stanag4529,
        Self::MilStd110a,
        Self::Stanag4539,
        Self::ThirtyNineTone,
        Self::SixteenTone,
        Self::Ale2g,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Stanag4285 => "STANAG 4285",
            Self::Stanag4529 => "STANAG 4529",
            Self::MilStd110a => "MIL-STD-188-110A",
            Self::Stanag4539 => "STANAG 4539",
            Self::ThirtyNineTone => "39-tone",
            Self::SixteenTone => "16-tone",
            Self::Ale2g => "2G ALE",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StanagInterleave {
    Zero,
    Short,
    Long,
}

impl StanagInterleave {
    pub const ALL: [Self; 3] = [Self::Zero, Self::Short, Self::Long];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Zero => "zero",
            Self::Short => "short",
            Self::Long => "long",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub struct StanagRate {
    pub bps: u16,
    pub interleave: StanagInterleave,
    #[serde(default)]
    pub voice: bool,
}

impl StanagRate {
    #[must_use]
    pub fn label(&self) -> String {
        let kind = if self.voice { " voice" } else { "" };
        format!("{} bps{kind} {}", self.bps, self.interleave.label())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct StanagReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modem: Option<StanagModem>,
    pub confidence: f32,
    pub audio_hz: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_hz: Option<f64>,
    pub bandwidth_hz: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate: Option<StanagRate>,
}

impl StanagReport {
    #[must_use]
    pub fn summary(&self) -> String {
        let Some(modem) = self.modem else {
            return "modem gone".to_owned();
        };
        let mut parts = vec![modem.label().to_owned()];
        if let Some(rate) = &self.rate {
            parts.push(rate.label());
        }
        parts.push(format!("{:.0} Hz", self.audio_hz));
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_settings_take_the_defaults() {
        let params: StanagParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params, StanagParams::default());
    }

    #[test]
    fn summary_names_the_modem_rate_and_carrier() {
        let report = StanagReport {
            modem: Some(StanagModem::MilStd110a),
            confidence: 0.9,
            audio_hz: 1_801.4,
            frequency_hz: None,
            bandwidth_hz: 2_700.0,
            rate: Some(StanagRate {
                bps: 1_200,
                interleave: StanagInterleave::Long,
                voice: false,
            }),
        };
        assert_eq!(
            report.summary(),
            "MIL-STD-188-110A · 1200 bps long · 1801 Hz"
        );
        let gone = StanagReport {
            modem: None,
            rate: None,
            ..report
        };
        assert_eq!(gone.summary(), "modem gone");
    }

    #[test]
    fn modems_have_readable_wire_names() {
        let names: Vec<_> = StanagModem::ALL
            .iter()
            .map(|modem| serde_json::to_value(modem).unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "stanag_4285",
                "stanag_4529",
                "mil_std_188_110a",
                "stanag_4539",
                "thirty_nine_tone",
                "sixteen_tone",
                "ale_2g",
            ]
        );
    }
}

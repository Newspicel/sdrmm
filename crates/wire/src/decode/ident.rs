use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Modulation {
    #[default]
    None,
    Carrier,
    Ook,
    Am,
    Ssb,
    Fm,
    Fsk2,
    Fsk4,
    Fsk8,
    Psk2,
    Psk4,
    Ofdm,
    NoiseLike,
    Unknown,
}

impl Modulation {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "no signal",
            Self::Carrier => "unmodulated carrier",
            Self::Ook => "OOK",
            Self::Am => "AM",
            Self::Ssb => "SSB",
            Self::Fm => "FM",
            Self::Fsk2 => "2-FSK",
            Self::Fsk4 => "4-FSK",
            Self::Fsk8 => "8-FSK",
            Self::Psk2 => "BPSK",
            Self::Psk4 => "QPSK",
            Self::Ofdm => "OFDM",
            Self::NoiseLike => "noise-like",
            Self::Unknown => "unknown",
        }
    }

    #[must_use]
    pub const fn is_signal(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentFeatures {
    pub envelope_variation: f32,
    pub duty: f32,
    pub keying_depth_db: f32,
    pub spectral_asymmetry: f32,
    pub carrier_db: f32,
    pub spectral_flatness: f32,
    pub frequency_levels: u8,
    pub frequency_spread_hz: f64,
    pub square_line_db: f32,
    pub quartic_line_db: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ProtocolMatch {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_id: Option<String>,
    pub score: f32,
    #[serde(default)]
    pub confirmed: bool,
    pub why: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentSignal {
    pub modulation: Modulation,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sideband: Option<crate::channel::Sideband>,
    /// Where the signal sits on the dial, in absolute Hz.
    pub frequency_hz: f64,
    pub center_offset_hz: f64,
    pub bandwidth_hz: f64,
    pub snr_db: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_rate_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deviation_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub burst_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub burst_period_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ofdm_symbol_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ofdm_guard_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<ProtocolMatch>,
    pub features: IdentFeatures,
}

impl IdentSignal {
    #[must_use]
    pub fn best(&self) -> Option<&ProtocolMatch> {
        self.candidates.first()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IdentReport {
    /// The loudest bin over the noise floor, measured even when nothing crossed the threshold.
    pub snr_db: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signals: Vec<IdentSignal>,
}

impl IdentReport {
    #[must_use]
    pub fn loudest(&self) -> Option<&IdentSignal> {
        self.signals.first()
    }

    #[must_use]
    pub fn best(&self) -> Option<&ProtocolMatch> {
        self.loudest().and_then(IdentSignal::best)
    }
}

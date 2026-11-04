use crate::default_true;
use serde::{Deserialize, Serialize};

use sdrmm_wire::labelled;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtvModulation {
    #[default]
    Am,
    Fm,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtvStandard {
    #[default]
    Ccir625,
    Eia525,
    SystemA405,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtvColor {
    #[default]
    Monochrome,
    Pal,
    Ntsc,
}

impl AtvStandard {
    #[must_use]
    pub fn lines(self) -> u16 {
        match self {
            Self::Ccir625 => 625,
            Self::Eia525 => 525,
            Self::SystemA405 => 405,
        }
    }

    #[must_use]
    pub fn line_rate_hz(self) -> f64 {
        match self {
            Self::Ccir625 => 15_625.0,
            Self::Eia525 => 15_734.264,
            Self::SystemA405 => 10_125.0,
        }
    }

    #[must_use]
    pub fn frame_rate_hz(self) -> f64 {
        self.line_rate_hz() / f64::from(self.lines())
    }
}

fn default_atv_bandwidth_hz() -> f64 {
    1_500_000.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AtvParams {
    #[serde(default)]
    pub modulation: AtvModulation,
    #[serde(default)]
    pub standard: AtvStandard,
    #[serde(default = "default_atv_bandwidth_hz")]
    pub bandwidth_hz: f64,
    #[serde(default)]
    pub invert: bool,
    #[serde(default = "default_true")]
    pub interlace: bool,
    #[serde(default)]
    pub color: AtvColor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound_subcarrier_hz: Option<f64>,
}

impl Default for AtvParams {
    fn default() -> Self {
        Self {
            modulation: AtvModulation::default(),
            standard: AtvStandard::default(),
            bandwidth_hz: default_atv_bandwidth_hz(),
            invert: false,
            interlace: true,
            color: AtvColor::default(),
            sound_subcarrier_hz: None,
        }
    }
}

labelled! {
    AtvModulation { Am => "AM", Fm => "FM" }
}

labelled! {
    AtvStandard { Ccir625 => "625 / 25", Eia525 => "525 / 30", SystemA405 => "405 / 25" }
}

labelled! {
    AtvColor { Monochrome => "Mono", Pal => "PAL", Ntsc => "NTSC" }
}

impl sdrmm_wire::DecoderSettings for AtvParams {
    const TYPE_ID: &'static str = "atv";
}

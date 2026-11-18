use sdrmm_modem::dvb::PunctureRate;
use serde::{Deserialize, Serialize};

use sdrmm_wire::SettingChoice;
use sdrmm_wire::labelled;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatvStandard {
    #[default]
    DvbS,
    DvbS2,
}

pub const MIN_DATV_SYMBOL_RATE: f64 = 100_000.0;

pub const MAX_DATV_SYMBOL_RATE: f64 = 4_000_000.0;

pub const MAX_SUPERFRAME_CODE: u32 = (1 << 20) - 2;

fn default_datv_symbol_rate() -> f64 {
    333_000.0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatvRollOff {
    #[default]
    Pct35,
    Pct25,
    Pct20,
    Pct15,
    Pct10,
    Pct5,
}

impl DatvRollOff {
    #[must_use]
    pub const fn factor(self) -> f64 {
        match self {
            Self::Pct35 => 0.35,
            Self::Pct25 => 0.25,
            Self::Pct20 => 0.20,
            Self::Pct15 => 0.15,
            Self::Pct10 => 0.10,
            Self::Pct5 => 0.05,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatvCodeRate {
    #[default]
    Auto,
    Half,
    TwoThirds,
    ThreeQuarters,
    FiveSixths,
    SevenEighths,
}

impl DatvCodeRate {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self.puncture() {
            None => "Auto",
            Some(rate) => rate.label(),
        }
    }

    /// The rate to decode with, `None` for `Auto`, which searches every rate.
    #[must_use]
    pub const fn puncture(self) -> Option<PunctureRate> {
        match self {
            Self::Auto => None,
            Self::Half => Some(PunctureRate::Half),
            Self::TwoThirds => Some(PunctureRate::TwoThirds),
            Self::ThreeQuarters => Some(PunctureRate::ThreeQuarters),
            Self::FiveSixths => Some(PunctureRate::FiveSixths),
            Self::SevenEighths => Some(PunctureRate::SevenEighths),
        }
    }
}

impl From<PunctureRate> for DatvCodeRate {
    fn from(rate: PunctureRate) -> Self {
        match rate {
            PunctureRate::Half => Self::Half,
            PunctureRate::TwoThirds => Self::TwoThirds,
            PunctureRate::ThreeQuarters => Self::ThreeQuarters,
            PunctureRate::FiveSixths => Self::FiveSixths,
            PunctureRate::SevenEighths => Self::SevenEighths,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DatvParams {
    #[serde(default)]
    pub superframes: bool,
    #[serde(default)]
    pub superframe_reference: u32,
    #[serde(default)]
    pub superframe_payload: u32,
    #[serde(default)]
    pub superframe_search: bool,
    #[serde(default)]
    pub standard: DatvStandard,
    #[serde(default = "default_datv_symbol_rate")]
    pub symbol_rate: f64,
    #[serde(default)]
    pub code_rate: DatvCodeRate,
    #[serde(default)]
    pub roll_off: DatvRollOff,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_stream: Option<u8>,
}

impl Default for DatvParams {
    fn default() -> Self {
        Self {
            standard: DatvStandard::default(),
            symbol_rate: default_datv_symbol_rate(),
            superframes: false,
            superframe_reference: 0,
            superframe_payload: 0,
            superframe_search: false,
            code_rate: DatvCodeRate::default(),
            roll_off: DatvRollOff::default(),
            program: None,
            input_stream: None,
        }
    }
}

labelled! {
    DatvStandard { DvbS => "DVB-S", DvbS2 => "DVB-S2" }
}

labelled! {
    DatvRollOff {
        Pct35 => "0.35", Pct25 => "0.25", Pct20 => "0.20", Pct15 => "0.15", Pct10 => "0.10",
        Pct5 => "0.05",
    }
}

impl SettingChoice for DatvCodeRate {
    fn all() -> &'static [Self] {
        &[
            Self::Auto,
            Self::Half,
            Self::TwoThirds,
            Self::ThreeQuarters,
            Self::FiveSixths,
            Self::SevenEighths,
        ]
    }

    fn choice_label(self) -> &'static str {
        self.label()
    }
}

/// The key a broadcast status carries the superframe found under.
pub const SUPERFRAME_KEY: &str = "superframe";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuperframeStatus {
    pub format: u8,
    pub sosf: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pilot: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trailer: Option<u8>,
    pub reference: u32,
    pub payload: u32,
}

impl SuperframeStatus {
    /// The superframe `status` reports, if it found one.
    #[must_use]
    pub fn of(status: &sdrmm_decoder::broadcast::BroadcastStatus) -> Option<Self> {
        status
            .extra
            .get(SUPERFRAME_KEY)
            .and_then(|found| serde_json::from_value(found.clone()).ok())
    }
}

impl sdrmm_wire::DecoderSettings for DatvParams {
    const TYPE_ID: &'static str = "datv";
}

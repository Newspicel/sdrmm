use serde::{Deserialize, Serialize};

use sdrmm_wire::labelled;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdacsBaud {
    #[default]
    Auto,
    B9600,
    B4800,
}

impl EdacsBaud {
    /// The rates a channel listens for: both until one is heard, when left to the signal.
    #[must_use]
    pub const fn rates(self) -> &'static [Self] {
        match self {
            Self::Auto => &[Self::B9600, Self::B4800],
            Self::B9600 => &[Self::B9600],
            Self::B4800 => &[Self::B4800],
        }
    }

    #[must_use]
    pub const fn hz(self) -> f64 {
        match self {
            Self::B4800 => 4_800.0,
            Self::Auto | Self::B9600 => 9_600.0,
        }
    }
}

/// How a site numbers its radios and talkgroups. Neither shows on the air, so the operator
/// names it; ESK systems also scramble each message with a fixed key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdacsAddressing {
    #[default]
    Standard,
    Extended,
    StandardEsk,
    ExtendedEsk,
}

impl EdacsAddressing {
    #[must_use]
    pub const fn extended(self) -> bool {
        matches!(self, Self::Extended | Self::ExtendedEsk)
    }

    #[must_use]
    pub const fn esk(self) -> bool {
        matches!(self, Self::StandardEsk | Self::ExtendedEsk)
    }
}

labelled! {
    EdacsBaud { Auto => "Auto", B9600 => "9600", B4800 => "4800" }
    EdacsAddressing {
        Standard => "Standard": "Standard or networked addressing",
        Extended => "Extended": "Extended addressing (EA)",
        StandardEsk => "Standard ESK",
        ExtendedEsk => "Extended ESK",
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdacsParams {
    #[serde(default)]
    pub baud: EdacsBaud,
    #[serde(default)]
    pub addressing: EdacsAddressing,
}

impl sdrmm_wire::DecoderSettings for EdacsParams {
    const TYPE_ID: &'static str = "edacs";
}

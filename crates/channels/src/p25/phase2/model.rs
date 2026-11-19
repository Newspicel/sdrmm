use serde::{Deserialize, Serialize};

use sdrmm_wire::labelled;

/// The logical channels of a Phase 2 carrier the decoder plays and reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P25p2Slots {
    #[default]
    Both,
    One,
    Two,
}

impl P25p2Slots {
    #[must_use]
    pub fn accepts(self, slot: u8) -> bool {
        match self {
            Self::Both => true,
            Self::One => slot == 1,
            Self::Two => slot == 2,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct P25p2Params {
    #[serde(default)]
    pub slots: P25p2Slots,
    /// The serial port of a DVSI vocoder chip that synthesizes the voice; unset is blip25.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocoder: Option<String>,
}

labelled! {
    P25p2Slots { Both => "Both", One => "TS1", Two => "TS2" }
}

impl sdrmm_wire::DecoderSettings for P25p2Params {
    const TYPE_ID: &'static str = "p25p2";
}

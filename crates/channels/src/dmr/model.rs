use serde::{Deserialize, Serialize};

use sdrmm_wire::{VoiceKey, labelled};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DmrSlots {
    #[default]
    Both,
    One,
    Two,
}

impl DmrSlots {
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
pub struct DmrParams {
    #[serde(default)]
    pub slots: DmrSlots,
    #[serde(default)]
    pub ignore_crc: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<VoiceKey>,
    /// The serial port of a DVSI vocoder chip that synthesizes the voice; unset is blip25.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocoder: Option<String>,
}

labelled! {
    DmrSlots { Both => "Both", One => "TS1", Two => "TS2" }
}

impl sdrmm_wire::DecoderSettings for DmrParams {
    const TYPE_ID: &'static str = "dmr";
}

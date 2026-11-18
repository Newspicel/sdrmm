use serde::{Deserialize, Serialize};

use sdrmm_wire::VoiceKey;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct P25Params {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<VoiceKey>,
    /// The serial port of a DVSI vocoder chip that synthesizes the voice; unset is blip25.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocoder: Option<String>,
}

impl sdrmm_wire::DecoderSettings for P25Params {
    const TYPE_ID: &'static str = "p25";
}

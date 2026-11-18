use serde::{Deserialize, Serialize};

use sdrmm_wire::{VoiceKey, labelled};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NxdnBandwidth {
    #[default]
    Narrow,
    Wide,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NxdnParams {
    #[serde(default)]
    pub bandwidth: NxdnBandwidth,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<VoiceKey>,
    /// The serial port of a DVSI vocoder chip that synthesizes the voice; unset is blip25.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocoder: Option<String>,
}

labelled! {
    NxdnBandwidth { Narrow => "6.25", Wide => "12.5" }
}

impl sdrmm_wire::DecoderSettings for NxdnParams {
    const TYPE_ID: &'static str = "nxdn";
}

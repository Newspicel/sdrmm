use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DpmrParams {
    /// The serial port of a DVSI vocoder chip that synthesizes the voice; unset is blip25.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocoder: Option<String>,
}

impl sdrmm_wire::DecoderSettings for DpmrParams {
    const TYPE_ID: &'static str = "dpmr";
}

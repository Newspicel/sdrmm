use sdrmm_modem::ofdm::dab::TransmissionMode;
use serde::{Deserialize, Serialize};

use sdrmm_wire::labelled;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DabMode {
    #[default]
    Auto,
    Dab,
    DabPlus,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DabTransmissionMode {
    Auto,
    #[default]
    I,
    Ii,
    Iii,
    Iv,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DabParams {
    #[serde(default)]
    pub transmission_mode: DabTransmissionMode,
    #[serde(default)]
    pub mode: DabMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_id: Option<u32>,
}

labelled! {
    DabMode { Auto => "Auto", Dab => "DAB", DabPlus => "DAB+" }
}

labelled! {
    DabTransmissionMode { Auto => "Auto", I => "I", Ii => "II", Iii => "III", Iv => "IV" }
}

/// `Auto` builds the mode I chain until a search picks the mode.
impl From<DabTransmissionMode> for TransmissionMode {
    fn from(mode: DabTransmissionMode) -> Self {
        match mode {
            DabTransmissionMode::Auto | DabTransmissionMode::I => Self::I,
            DabTransmissionMode::Ii => Self::Ii,
            DabTransmissionMode::Iii => Self::Iii,
            DabTransmissionMode::Iv => Self::Iv,
        }
    }
}

impl sdrmm_wire::DecoderSettings for DabParams {
    const TYPE_ID: &'static str = "dab";
}

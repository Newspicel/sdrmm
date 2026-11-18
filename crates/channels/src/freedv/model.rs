use serde::{Deserialize, Serialize};

use sdrmm_wire::Sideband;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreeDvMode {
    #[default]
    Mode1600,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreeDvParams {
    #[serde(default)]
    pub mode: FreeDvMode,
    #[serde(default)]
    pub sideband: Sideband,
}

impl sdrmm_wire::DecoderSettings for FreeDvParams {
    const TYPE_ID: &'static str = "freedv";
}

use serde::{Deserialize, Serialize};

use sdrmm_wire::labelled;
use sdrmm_wire::{DecoderSettings, Sideband};

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

/// An enhanced privacy key as the radios are programmed with it: the PI header's key ID and the key
/// in hex, 10 digits for ARC4, 32 or 64 for AES.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DmrKey {
    pub key_id: u8,
    pub key: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DmrParams {
    #[serde(default)]
    pub slots: DmrSlots,
    #[serde(default)]
    pub ignore_crc: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<DmrKey>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NxdnBandwidth {
    #[default]
    Narrow,
    Wide,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NxdnParams {
    #[serde(default)]
    pub bandwidth: NxdnBandwidth,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DstarParams {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct YsfParams {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct P25Params {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DpmrParams {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct M17Params {}

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

labelled! {
    DmrSlots { Both => "Both", One => "TS1", Two => "TS2" }
}

labelled! {
    NxdnBandwidth { Narrow => "6.25", Wide => "12.5" }
}

impl DecoderSettings for DmrParams {
    const TYPE_ID: &'static str = "dmr";
}

impl DecoderSettings for NxdnParams {
    const TYPE_ID: &'static str = "nxdn";
}

impl DecoderSettings for DstarParams {
    const TYPE_ID: &'static str = "dstar";
}

impl DecoderSettings for YsfParams {
    const TYPE_ID: &'static str = "ysf";
}

impl DecoderSettings for P25Params {
    const TYPE_ID: &'static str = "p25";
}

impl DecoderSettings for DpmrParams {
    const TYPE_ID: &'static str = "dpmr";
}

impl DecoderSettings for M17Params {
    const TYPE_ID: &'static str = "m17";
}

impl DecoderSettings for FreeDvParams {
    const TYPE_ID: &'static str = "freedv";
}

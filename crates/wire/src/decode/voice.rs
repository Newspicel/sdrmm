use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{EventContract, EventKind};

/// How a digital voice mode names its colour code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CodeStyle {
    #[default]
    ColorCode,
    Nac,
    Ran,
}

/// How a digital voice mode names its network, system and site identifiers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SiteNaming {
    #[default]
    Network,
    Cellular,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DvFrameKind {
    #[default]
    Header,
    Voice,
    Terminator,
    Control,
    Data,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Vendor {
    Standard,
    Etsi,
    Motorola,
    Hytera,
    Harris,
    Tait,
    JvcKenwood,
    Emc,
    RadioActivity,
    FlydeMicro,
    ProdEl,
    Unknown,
}

impl Vendor {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Etsi => "ETSI",
            Self::Motorola => "Motorola",
            Self::Hytera => "Hytera",
            Self::Harris => "Harris",
            Self::Tait => "Tait",
            Self::JvcKenwood => "JVCKENWOOD",
            Self::Emc => "EMC",
            Self::RadioActivity => "Radio Activity",
            Self::FlydeMicro => "Flyde Micro",
            Self::ProdEl => "PROD-EL",
            Self::Unknown => "unknown vendor",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DvSlotActivity {
    pub slot: u8,
    pub activity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_hash: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logical_channel: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DvChannelDefinition {
    pub channel: u16,
    pub tx_hz: u64,
    pub rx_hz: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_code: Option<u8>,
}

pub const MAX_VOICE_KEYS: usize = 256;
pub const MAX_VOICE_KEY_DIGITS: usize = 64;

/// A voice privacy key as the radios are programmed with it: the key ID a call names and the key
/// in hex. The decoder reads the algorithm off its length.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct VoiceKey {
    pub key_id: u16,
    pub key: String,
}

impl VoiceKey {
    #[must_use]
    pub fn valid(&self) -> bool {
        (1..=MAX_VOICE_KEY_DIGITS).contains(&self.key.len())
            && self.key.bytes().all(|byte| byte.is_ascii_hexdigit())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DvFrame {
    /// The mode's display name, as its decoder states it.
    pub mode: String,
    #[serde(default)]
    pub code_style: CodeStyle,
    #[serde(default)]
    pub site_naming: SiteNaming,
    pub kind: DvFrameKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_code: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<Vendor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer_id: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_call: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_call: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_call: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub algorithm_id: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_indicator: Option<String>,
    /// The cipher the receiver is undoing on the voice, once it can.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cipher: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub talker_alias: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_error_m: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_definition: Option<DvChannelDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The id of a trunking protocol the decoder declares in its [`crate::TrunkSpec`].
    pub trunk_protocol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trunk_label: Option<String>,
    /// Whether the burst came from a trunked site's control channel rather than one of its
    /// traffic channels, where the air interface says so outright.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_channel: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crc_verified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest_channel: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_id: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_id: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emergency: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub late_entry: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slot_activity: Vec<DvSlotActivity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opcode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub errors_corrected: u32,
}

impl DvFrame {
    #[must_use]
    pub fn new(mode: &str, kind: DvFrameKind) -> Self {
        Self {
            mode: mode.to_owned(),
            kind,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn parties(&self) -> Option<String> {
        let to = self.destination_call.clone().or_else(|| {
            self.destination.map(|d| match self.group_call {
                Some(false) => d.to_string(),
                _ => format!("TG {d}"),
            })
        });
        let from = self
            .source_call
            .clone()
            .or_else(|| self.source.map(|s| s.to_string()));
        match (to, from) {
            (Some(to), Some(from)) => Some(format!("{to} ← {from}")),
            (Some(to), None) => Some(to),
            (None, Some(from)) => Some(from),
            (None, None) => None,
        }
    }
}

impl EventContract for DvFrame {
    const KIND: EventKind = EventKind::DV;
}

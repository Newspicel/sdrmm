use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ToneSquelchStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctcss_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dcs_code: Option<u16>,
    pub open: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ScramblerStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inversion_hz: Option<f64>,
    pub confidence: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DvMode {
    #[default]
    Dmr,
    Dstar,
    Ysf,
    Nxdn,
    P25,
    Dpmr,
    M17,
    #[serde(rename = "freedv")]
    FreeDv,
    Tetra,
}

impl DvMode {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Dmr => "DMR",
            Self::Dstar => "D-STAR",
            Self::Ysf => "YSF",
            Self::Nxdn => "NXDN",
            Self::P25 => "P25",
            Self::Dpmr => "dPMR",
            Self::M17 => "M17",
            Self::FreeDv => "FreeDV",
            Self::Tetra => "TETRA",
        }
    }

    #[must_use]
    pub fn type_id(self) -> &'static str {
        match self {
            Self::Dmr => "dmr",
            Self::Dstar => "dstar",
            Self::Ysf => "ysf",
            Self::Nxdn => "nxdn",
            Self::P25 => "p25",
            Self::Dpmr => "dpmr",
            Self::M17 => "m17",
            Self::FreeDv => "freedv",
            Self::Tetra => "tetra",
        }
    }

    #[must_use]
    pub fn from_type_id(type_id: &str) -> Option<Self> {
        [
            Self::Dmr,
            Self::Dstar,
            Self::Ysf,
            Self::Nxdn,
            Self::P25,
            Self::Dpmr,
            Self::M17,
            Self::FreeDv,
            Self::Tetra,
        ]
        .into_iter()
        .find(|mode| mode.type_id() == type_id)
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DvTrunkProtocol {
    CapacityPlus,
    HyteraXpt,
    TierThree,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DvFrame {
    pub mode: DvMode,
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
    pub trunk_protocol: Option<DvTrunkProtocol>,
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
    pub fn new(mode: DvMode, kind: DvFrameKind) -> Self {
        Self {
            mode,
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

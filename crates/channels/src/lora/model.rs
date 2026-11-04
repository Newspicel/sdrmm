use sdrmm_wire::{Capability, EventContract, Hue, KindSpec, Symbol};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;
use sdrmm_wire::SettingChoice;

use sdrmm_wire::labelled;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Place;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::lat_lon;

labelled! {
    LoraBandwidth {
        Khz7_8 => "7.8 kHz", Khz10_4 => "10.4 kHz", Khz15_6 => "15.6 kHz", Khz20_8 => "20.8 kHz",
        Khz31_25 => "31.25 kHz", Khz41_7 => "41.7 kHz", Khz62_5 => "62.5 kHz",
        Khz125 => "125 kHz", Khz250 => "250 kHz", Khz500 => "500 kHz",
    }
}

labelled! {
    LoraSpreadingFactor {
        All => "All": "Search SF7 to SF12 at once",
        Sf7 => "SF7", Sf8 => "SF8", Sf9 => "SF9", Sf10 => "SF10", Sf11 => "SF11", Sf12 => "SF12",
    }
}

labelled! {
    LoraIq {
        Normal => "Normal",
        Inverted => "Inverted": "LoRaWAN downlinks",
        Both => "Both",
    }
}

impl SettingChoice for LoraCodingRate {
    fn all() -> &'static [Self] {
        &[Self::Cr45, Self::Cr46, Self::Cr47, Self::Cr48]
    }

    fn choice_label(self) -> &'static str {
        self.label()
    }
}

impl SettingChoice for LoraProtocol {
    fn all() -> &'static [Self] {
        &[
            Self::Auto,
            Self::Raw,
            Self::Lorawan,
            Self::Meshtastic,
            Self::Meshcore,
        ]
    }

    fn choice_label(self) -> &'static str {
        self.label()
    }

    fn choice_title(self) -> Option<&'static str> {
        (self == Self::Raw).then_some("Bytes only")
    }
}

mod fields;

pub const MAX_LORA_KEYS: usize = 32;

pub const MAX_LORA_KEY_NAME_LEN: usize = 32;

pub const MAX_LORA_KEY_TEXT_LEN: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoraBandwidth {
    Khz7_8,
    Khz10_4,
    Khz15_6,
    Khz20_8,
    Khz31_25,
    Khz41_7,
    Khz62_5,
    #[default]
    Khz125,
    Khz250,
    Khz500,
}

impl LoraBandwidth {
    pub const ALL: [Self; 10] = [
        Self::Khz7_8,
        Self::Khz10_4,
        Self::Khz15_6,
        Self::Khz20_8,
        Self::Khz31_25,
        Self::Khz41_7,
        Self::Khz62_5,
        Self::Khz125,
        Self::Khz250,
        Self::Khz500,
    ];

    #[must_use]
    pub fn hz(self) -> f64 {
        match self {
            Self::Khz7_8 => 500_000.0 / 64.0,
            Self::Khz10_4 => 500_000.0 / 48.0,
            Self::Khz15_6 => 500_000.0 / 32.0,
            Self::Khz20_8 => 500_000.0 / 24.0,
            Self::Khz31_25 => 500_000.0 / 16.0,
            Self::Khz41_7 => 500_000.0 / 12.0,
            Self::Khz62_5 => 500_000.0 / 8.0,
            Self::Khz125 => 500_000.0 / 4.0,
            Self::Khz250 => 500_000.0 / 2.0,
            Self::Khz500 => 500_000.0,
        }
    }

    #[must_use]
    pub fn nearest(hz: f64) -> Self {
        Self::ALL
            .into_iter()
            .min_by(|a, b| (a.hz() - hz).abs().total_cmp(&(b.hz() - hz).abs()))
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoraSpreadingFactor {
    #[default]
    All,
    Sf7,
    Sf8,
    Sf9,
    Sf10,
    Sf11,
    Sf12,
}

impl LoraSpreadingFactor {
    pub const MIN: u8 = 7;
    pub const MAX: u8 = 12;

    #[must_use]
    pub fn factors(self) -> std::ops::RangeInclusive<u8> {
        match self {
            Self::All => Self::MIN..=Self::MAX,
            Self::Sf7 => 7..=7,
            Self::Sf8 => 8..=8,
            Self::Sf9 => 9..=9,
            Self::Sf10 => 10..=10,
            Self::Sf11 => 11..=11,
            Self::Sf12 => 12..=12,
        }
    }

    #[must_use]
    pub fn of_chips(chips: f64) -> Self {
        match chips.log2().round() as i32 {
            7 => Self::Sf7,
            8 => Self::Sf8,
            9 => Self::Sf9,
            10 => Self::Sf10,
            11 => Self::Sf11,
            12 => Self::Sf12,
            _ => Self::All,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LoraCodingRate {
    #[default]
    #[serde(rename = "4/5")]
    Cr45,
    #[serde(rename = "4/6")]
    Cr46,
    #[serde(rename = "4/7")]
    Cr47,
    #[serde(rename = "4/8")]
    Cr48,
}

impl LoraCodingRate {
    #[must_use]
    pub fn parity_bits(self) -> u8 {
        match self {
            Self::Cr45 => 1,
            Self::Cr46 => 2,
            Self::Cr47 => 3,
            Self::Cr48 => 4,
        }
    }

    #[must_use]
    pub fn from_parity_bits(bits: u8) -> Option<Self> {
        match bits {
            1 => Some(Self::Cr45),
            2 => Some(Self::Cr46),
            3 => Some(Self::Cr47),
            4 => Some(Self::Cr48),
            _ => None,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Cr45 => "4/5",
            Self::Cr46 => "4/6",
            Self::Cr47 => "4/7",
            Self::Cr48 => "4/8",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoraIq {
    #[default]
    Normal,
    Inverted,
    Both,
}

impl LoraIq {
    #[must_use]
    pub fn listens(self, inverted: bool) -> bool {
        match self {
            Self::Normal => !inverted,
            Self::Inverted => inverted,
            Self::Both => true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoraProtocol {
    #[default]
    Auto,
    Raw,
    Lorawan,
    Meshtastic,
    Meshcore,
}

impl LoraProtocol {
    pub const LORAWAN_SYNC_WORD: u8 = 0x34;
    pub const MESHTASTIC_SYNC_WORD: u8 = 0x2b;
    pub const PRIVATE_SYNC_WORD: u8 = 0x12;

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Raw => "LoRa",
            Self::Lorawan => "LoRaWAN",
            Self::Meshtastic => "Meshtastic",
            Self::Meshcore => "MeshCore",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoraImplicitHeader {
    pub length: u8,
    #[serde(default)]
    pub coding_rate: LoraCodingRate,
    #[serde(default = "crate::default_true")]
    pub crc: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoraKey {
    MeshtasticChannel {
        name: String,
        psk: String,
    },
    MeshcoreChannel {
        name: String,
        secret: String,
    },
    LorawanSession {
        dev_addr: String,
        nwk_s_key: String,
        app_s_key: String,
    },
    LorawanAppKey {
        app_key: String,
    },
}

/// The `kind` tag a [`LoraKey`] carries on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoraKeyKind {
    MeshtasticChannel,
    MeshcoreChannel,
    LorawanSession,
    LorawanAppKey,
}

labelled! {
    LoraKeyKind {
        MeshtasticChannel => "Meshtastic": "Channel name and base64 PSK",
        MeshcoreChannel => "MeshCore": "Channel name and hex secret",
        LorawanSession => "ABP": "LoRaWAN DevAddr, NwkSKey and AppSKey",
        LorawanAppKey => "OTAA": "LoRaWAN AppKey",
    }
}

pub struct LoraKeyField<'a> {
    pub name: &'static str,
    pub text: &'a str,
    pub max_len: usize,
}

const fn field<'a>(name: &'static str, text: &'a str, max_len: usize) -> LoraKeyField<'a> {
    LoraKeyField {
        name,
        text,
        max_len,
    }
}

impl LoraKey {
    #[must_use]
    pub fn kind(&self) -> LoraKeyKind {
        match self {
            Self::MeshtasticChannel { .. } => LoraKeyKind::MeshtasticChannel,
            Self::MeshcoreChannel { .. } => LoraKeyKind::MeshcoreChannel,
            Self::LorawanSession { .. } => LoraKeyKind::LorawanSession,
            Self::LorawanAppKey { .. } => LoraKeyKind::LorawanAppKey,
        }
    }

    #[must_use]
    pub fn fields(&self) -> Vec<LoraKeyField<'_>> {
        match self {
            Self::MeshtasticChannel { name, psk } => vec![
                field("name", name, MAX_LORA_KEY_NAME_LEN),
                field("key", psk, MAX_LORA_KEY_TEXT_LEN),
            ],
            Self::MeshcoreChannel { name, secret } => vec![
                field("name", name, MAX_LORA_KEY_NAME_LEN),
                field("secret", secret, MAX_LORA_KEY_TEXT_LEN),
            ],
            Self::LorawanSession {
                dev_addr,
                nwk_s_key,
                app_s_key,
            } => vec![
                field("device address", dev_addr, MAX_LORA_KEY_TEXT_LEN),
                field("network session key", nwk_s_key, MAX_LORA_KEY_TEXT_LEN),
                field("app session key", app_s_key, MAX_LORA_KEY_TEXT_LEN),
            ],
            Self::LorawanAppKey { app_key } => {
                vec![field("app key", app_key, MAX_LORA_KEY_TEXT_LEN)]
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoraParams {
    #[serde(default)]
    pub bandwidth: LoraBandwidth,
    #[serde(default)]
    pub spreading_factor: LoraSpreadingFactor,
    #[serde(default)]
    pub iq: LoraIq,
    #[serde(default)]
    pub protocol: LoraProtocol,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "implicit_header"
    )]
    pub implicit_header: Option<LoraImplicitHeader>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<LoraKey>,
}

/// A header whose length a form has cleared means an explicit header.
fn implicit_header<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<LoraImplicitHeader>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        Some(header) if header.get("length").is_some_and(|length| !length.is_null()) => {
            serde_json::from_value(header)
                .map(Some)
                .map_err(serde::de::Error::custom)
        }
        _ => Ok(None),
    }
}

impl LoraParams {
    pub fn validate(&self) -> Result<(), String> {
        if self.keys.len() > MAX_LORA_KEYS {
            return Err(format!("at most {MAX_LORA_KEYS} LoRa keys"));
        }
        let over = self
            .keys
            .iter()
            .flat_map(LoraKey::fields)
            .find(|field| field.text.chars().count() > field.max_len);
        if let Some(field) = over {
            return Err(format!(
                "a LoRa {} is at most {} characters, got {}",
                field.name,
                field.max_len,
                field.text.chars().count()
            ));
        }
        if self
            .implicit_header
            .is_some_and(|header| header.length == 0)
        {
            return Err("an implicit LoRa header needs a payload length".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoraIntegrity {
    CrcOk,
    CrcFailed,
    NoCrc,
    HeaderFailed,
}

impl LoraIntegrity {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::CrcOk => "CRC ok",
            Self::CrcFailed => "CRC failed",
            Self::NoCrc => "no CRC",
            Self::HeaderFailed => "header failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoraFrame {
    pub spreading_factor: u8,
    pub bandwidth_hz: f64,
    pub coding_rate: LoraCodingRate,
    pub sync_word: u8,
    pub implicit_header: bool,
    pub low_data_rate: bool,
    pub inverted_iq: bool,
    pub integrity: LoraIntegrity,
    pub fec_corrected: u32,
    pub snr_db: f32,
    pub frequency_error_hz: f32,
    pub payload: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoded: Option<LoraPayload>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "protocol", rename_all = "snake_case")]
pub enum LoraPayload {
    Lorawan(LorawanFrame),
    Meshtastic(MeshtasticPacket),
    Meshcore(MeshcorePacket),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LorawanMessageType {
    JoinRequest,
    JoinAccept,
    UnconfirmedUp,
    UnconfirmedDown,
    ConfirmedUp,
    ConfirmedDown,
    RejoinRequest,
    Proprietary,
}

impl LorawanMessageType {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::JoinRequest => "Join request",
            Self::JoinAccept => "Join accept",
            Self::UnconfirmedUp => "Uplink",
            Self::UnconfirmedDown => "Downlink",
            Self::ConfirmedUp => "Confirmed uplink",
            Self::ConfirmedDown => "Confirmed downlink",
            Self::RejoinRequest => "Rejoin request",
            Self::Proprietary => "Proprietary",
        }
    }

    #[must_use]
    pub fn uplink(self) -> bool {
        matches!(
            self,
            Self::JoinRequest | Self::UnconfirmedUp | Self::ConfirmedUp | Self::RejoinRequest
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LorawanFrame {
    pub message_type: LorawanMessageType,
    pub major: u8,
    pub mic: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mic_ok: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_addr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adr: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adr_ack_req: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ack: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_or_class_b: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub f_cnt: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub f_port: Option<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mac_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frm_payload: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decrypted: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_eui: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_eui: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_nonce: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_nonce: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rx1_dr_offset: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rx2_data_rate: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rx_delay_s: Option<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cf_list_hz: Vec<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeshtasticEncryption {
    Open,
    Channel,
    Pki,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshtasticPacket {
    pub to: u32,
    pub from: u32,
    pub id: u32,
    pub hop_limit: u8,
    pub hop_start: u8,
    pub want_ack: bool,
    pub via_mqtt: bool,
    pub channel_hash: u8,
    pub next_hop: u8,
    pub relay_node: u8,
    pub encryption: MeshtasticEncryption,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<MeshtasticContent>,
}

impl MeshtasticPacket {
    pub const BROADCAST: u32 = 0xffff_ffff;

    #[must_use]
    pub fn node_id(node: u32) -> String {
        if node == Self::BROADCAST {
            "^all".to_owned()
        } else {
            format!("!{node:08x}")
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshtasticMetric {
    pub name: String,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshtasticNeighbor {
    pub node: u32,
    pub snr_db: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MeshtasticContent {
    Text {
        text: String,
    },
    Position {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lat: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lon: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        altitude_m: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        time: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        satellites: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ground_speed_kmh: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        precision_bits: Option<u32>,
    },
    NodeInfo {
        id: String,
        long_name: String,
        short_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hw_model: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<u32>,
        licensed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        public_key: Option<String>,
    },
    Telemetry {
        kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        time: Option<u32>,
        metrics: Vec<MeshtasticMetric>,
    },
    Routing {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        route: Vec<u32>,
    },
    Traceroute {
        route: Vec<u32>,
        snr_towards_db: Vec<f32>,
        route_back: Vec<u32>,
        snr_back_db: Vec<f32>,
    },
    NeighborInfo {
        node: u32,
        neighbors: Vec<MeshtasticNeighbor>,
    },
    Waypoint {
        id: u32,
        name: String,
        description: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lat: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lon: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expire: Option<u32>,
    },
    MapReport {
        long_name: String,
        short_name: String,
        firmware_version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lat: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lon: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        altitude_m: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        online_nodes: Option<u32>,
    },
    Data {
        bytes: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeshcoreRoute {
    TransportFlood,
    Flood,
    Direct,
    TransportDirect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeshcorePayloadType {
    Request,
    Response,
    Text,
    Ack,
    Advert,
    GroupText,
    GroupData,
    AnonRequest,
    Path,
    Trace,
    Multipart,
    Control,
    Reserved,
    RawCustom,
}

impl MeshcorePayloadType {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Request => "Request",
            Self::Response => "Response",
            Self::Text => "Text",
            Self::Ack => "Ack",
            Self::Advert => "Advert",
            Self::GroupText => "Group text",
            Self::GroupData => "Group data",
            Self::AnonRequest => "Anon request",
            Self::Path => "Path",
            Self::Trace => "Trace",
            Self::Multipart => "Multipart",
            Self::Control => "Control",
            Self::Reserved => "Reserved",
            Self::RawCustom => "Custom",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeshcoreNodeType {
    None,
    Chat,
    Repeater,
    Room,
    Sensor,
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshcorePacket {
    pub route: MeshcoreRoute,
    pub payload_type: MeshcorePayloadType,
    pub version: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transport_codes: Vec<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
    pub content: MeshcoreContent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MeshcoreContent {
    Advert {
        public_key: String,
        timestamp: u32,
        node_type: MeshcoreNodeType,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lat: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lon: Option<f64>,
        signature_ok: bool,
    },
    GroupText {
        channel_hash: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timestamp: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sender: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    GroupData {
        channel_hash: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data_type: Option<u16>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<String>,
    },
    Encrypted {
        destination: String,
        source: String,
        mac: String,
    },
    AnonRequest {
        destination: String,
        public_key: String,
    },
    Ack {
        checksum: u32,
    },
    Trace {
        tag: u32,
        auth_code: u32,
        flags: u8,
        hops: Vec<String>,
    },
    Control {
        subtype: u8,
        data: String,
    },
    Multipart {
        remaining: u8,
        inner_type: u8,
        data: String,
    },
    Raw {
        data: String,
    },
}

impl LoraFrame {
    #[must_use]
    pub fn station(&self) -> Option<String> {
        match self.decoded.as_ref()? {
            LoraPayload::Lorawan(frame) => frame.dev_addr.clone().or_else(|| frame.dev_eui.clone()),
            LoraPayload::Meshtastic(packet) => Some(MeshtasticPacket::node_id(packet.from)),
            LoraPayload::Meshcore(packet) => match &packet.content {
                MeshcoreContent::Advert {
                    name, public_key, ..
                } => name
                    .clone()
                    .or_else(|| Some(public_key.chars().take(8).collect())),
                MeshcoreContent::GroupText { sender, .. } => sender.clone(),
                MeshcoreContent::Encrypted { source, .. } => Some(source.clone()),
                _ => None,
            },
        }
    }

    #[must_use]
    pub fn position(&self) -> Option<(f64, f64)> {
        match self.decoded.as_ref()? {
            LoraPayload::Meshtastic(packet) => match packet.content.as_ref()? {
                MeshtasticContent::Position { lat, lon, .. }
                | MeshtasticContent::Waypoint { lat, lon, .. }
                | MeshtasticContent::MapReport { lat, lon, .. } => lat.zip(*lon),
                _ => None,
            },
            LoraPayload::Meshcore(packet) => match &packet.content {
                MeshcoreContent::Advert { lat, lon, .. } => lat.zip(*lon),
                _ => None,
            },
            LoraPayload::Lorawan(_) => None,
        }
    }

    #[must_use]
    pub fn name(&self) -> Option<String> {
        match self.decoded.as_ref()? {
            LoraPayload::Meshtastic(packet) => match packet.content.as_ref()? {
                MeshtasticContent::NodeInfo { long_name, .. }
                | MeshtasticContent::MapReport { long_name, .. } => {
                    sdrmm_wire::present::trimmed(Some(long_name))
                }
                _ => None,
            },
            LoraPayload::Meshcore(packet) => match &packet.content {
                MeshcoreContent::Advert { name, .. } => name.clone(),
                _ => None,
            },
            LoraPayload::Lorawan(_) => None,
        }
    }

    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = vec![format!(
            "SF{} {}",
            self.spreading_factor,
            sdrmm_wire::units::hertz(self.bandwidth_hz)
        )];
        match &self.decoded {
            Some(LoraPayload::Lorawan(frame)) => parts.extend(lorawan_summary(frame)),
            Some(LoraPayload::Meshtastic(packet)) => parts.extend(meshtastic_summary(packet)),
            Some(LoraPayload::Meshcore(packet)) => parts.extend(meshcore_summary(packet)),
            None => parts.push(format!("{} bytes", self.payload.len() / 2)),
        }
        if self.integrity != LoraIntegrity::CrcOk {
            parts.push(self.integrity.label().to_owned());
        }
        parts.join(" · ")
    }
}

fn lorawan_summary(frame: &LorawanFrame) -> Vec<String> {
    let mut parts = vec![frame.message_type.label().to_owned()];
    if let Some(addr) = &frame.dev_addr {
        parts.push(addr.clone());
    }
    if let Some(eui) = &frame.dev_eui {
        parts.push(format!("DevEUI {eui}"));
    }
    if let Some(count) = frame.f_cnt {
        parts.push(format!("FCnt {count}"));
    }
    if let Some(port) = frame.f_port {
        parts.push(format!("port {port}"));
    }
    if !frame.mac_commands.is_empty() {
        parts.push(frame.mac_commands.join(", "));
    }
    parts
}

fn meshtastic_summary(packet: &MeshtasticPacket) -> Vec<String> {
    let mut parts = vec![format!(
        "{} → {}",
        MeshtasticPacket::node_id(packet.from),
        MeshtasticPacket::node_id(packet.to)
    )];
    match (&packet.content, packet.encryption) {
        (Some(content), _) => parts.push(meshtastic_content_summary(content)),
        (None, MeshtasticEncryption::Pki) => parts.push("direct message".to_owned()),
        (None, _) => parts.push("encrypted".to_owned()),
    }
    parts
}

fn meshtastic_content_summary(content: &MeshtasticContent) -> String {
    match content {
        MeshtasticContent::Text { text } => text.clone(),
        MeshtasticContent::Position { lat, lon, .. } => match lat.zip(*lon) {
            Some((lat, lon)) => format!("position {lat:.5}, {lon:.5}"),
            None => "position".to_owned(),
        },
        MeshtasticContent::NodeInfo {
            long_name,
            short_name,
            ..
        } => format!("{long_name} ({short_name})"),
        MeshtasticContent::Telemetry { kind, metrics, .. } => {
            let values: Vec<String> = metrics
                .iter()
                .take(3)
                .map(|metric| format!("{} {:.1}", metric.name, metric.value))
                .collect();
            format!("{kind} telemetry {}", values.join(", "))
        }
        MeshtasticContent::Routing { error, .. } => match error {
            Some(error) => format!("routing {error}"),
            None => "ack".to_owned(),
        },
        MeshtasticContent::Traceroute { route, .. } => format!("traceroute {} hops", route.len()),
        MeshtasticContent::NeighborInfo { neighbors, .. } => {
            format!("{} neighbors", neighbors.len())
        }
        MeshtasticContent::Waypoint { name, .. } => format!("waypoint {name}"),
        MeshtasticContent::MapReport { long_name, .. } => format!("map report {long_name}"),
        MeshtasticContent::Data { bytes } => format!("{} bytes", bytes.len() / 2),
    }
}

fn meshcore_summary(packet: &MeshcorePacket) -> Vec<String> {
    let mut parts = vec![packet.payload_type.label().to_owned()];
    match &packet.content {
        MeshcoreContent::Advert { name, .. } => parts.extend(name.clone()),
        MeshcoreContent::GroupText {
            channel,
            sender,
            text,
            ..
        } => {
            parts.extend(channel.clone());
            match (sender, text) {
                (Some(sender), Some(text)) => parts.push(format!("{sender}: {text}")),
                (None, Some(text)) => parts.push(text.clone()),
                _ => parts.push("encrypted".to_owned()),
            }
        }
        MeshcoreContent::Encrypted {
            destination,
            source,
            ..
        } => parts.push(format!("{source} → {destination}")),
        _ => {}
    }
    if !packet.path.is_empty() {
        parts.push(format!("{} hops", packet.path.len()));
    }
    parts
}

impl EventPayload for LoraFrame {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let (lat, lon) = self.position().unzip();
        Presentation {
            entity: self.station(),
            label: self.name(),
            place: lat_lon(lat, lon).map(Place::at),
            fields: self.fields(),
            verified: Some(matches!(
                self.integrity,
                LoraIntegrity::CrcOk | LoraIntegrity::NoCrc
            )),
            ..Presentation::of(self.summary())
        }
    }

    fn merge(&mut self, newer: &Self) {
        if newer.position().is_some() || self.position().is_none() {
            *self = newer.clone();
            return;
        }
        let decoded = self.decoded.take();
        *self = Self {
            decoded,
            ..newer.clone()
        };
    }
}

pub(crate) fn lora_kind() -> KindSpec {
    KindSpec::of::<LoraFrame>("LoRa", "Nodes", Hue::Green)
        .caps(&[Capability::Log, Capability::Station, Capability::Geo])
        .symbol(Symbol::Node)
        .sighting()
        .build()
}

impl EventContract for LoraFrame {
    const KIND: EventKind = EventKind::named("lora");
}

impl sdrmm_wire::DecoderSettings for LoraParams {
    const TYPE_ID: &'static str = "lora";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use crate::cw_skimmer::CwSkimmerSpot;
    use crate::gnss::GnssFrame;
    use crate::weak_signal::WsjtMessage;
    use crate::weak_signal::WsprSpot;
    use sdrmm_wire::present::count;
    use sdrmm_wire::present::text;

    use sdrmm_wire::IdentReport;

    use sdrmm_wire::Transmission;
    use sdrmm_wire::VoiceCall;

    use sdrmm_wire::present::FieldValue;
    use sdrmm_wire::present::Unit;

    use sdrmm_wire::present::number;
    use sdrmm_wire::present::only_if;
    use sdrmm_wire::present::percent;
    use sdrmm_wire::present::text_of;

    #[test]
    fn a_quiet_report_names_the_loudest_bin() {
        let view = IdentReport {
            snr_db: 4.25,
            signals: Vec::new(),
        }
        .present(&EventKind::IDENT);
        assert_eq!(view.labels(), ["Modulation", "Loudest bin"]);
        assert_eq!(view.field("Modulation"), text_of("no signal").as_ref());
    }

    #[test]
    fn the_loudest_signal_leads_and_candidates_close() {
        let report: IdentReport = serde_json::from_value(serde_json::json!({
            "snr_db": 30.0,
            "signals": [{
                "modulation": "fsk2", "confidence": 0.82, "frequency_hz": 433.92e6,
                "center_offset_hz": 1200.0, "bandwidth_hz": 25_000.0, "snr_db": 21.5,
                "symbol_rate_hz": 4800.0, "deviation_hz": 9600.0,
                "candidates": [{ "name": "POCSAG", "score": 0.7, "why": "rate fits" }],
                "features": {
                    "envelope_variation": 0.0, "duty": 0.5, "keying_depth_db": 0.0,
                    "spectral_asymmetry": 0.0, "carrier_db": 0.0, "spectral_flatness": 0.0,
                    "frequency_levels": 2, "frequency_spread_hz": 0.0, "square_line_db": 0.0,
                    "quartic_line_db": 0.0,
                },
            }],
        }))
        .unwrap();
        let view = report.present(&EventKind::IDENT);
        assert_eq!(view.field("Signals"), count(Some(1)).as_ref());
        assert_eq!(view.field("Modulation"), text_of("2-FSK").as_ref());
        assert_eq!(view.field("Confidence"), percent(0.82).as_ref());
        assert_eq!(
            view.field("Symbol rate"),
            number(Some(4800.0), Unit::Baud, 0).as_ref()
        );
        assert_eq!(view.field("Duty"), percent(0.5).as_ref());
        assert_eq!(view.field("Frequency levels"), count(Some(2)).as_ref());
        assert_eq!(
            view.field("Candidates"),
            text_of("433.92 MHz · 2-FSK\nPOCSAG: 70%, rate fits").as_ref()
        );
    }

    #[test]
    fn gnss_says_when_it_is_still_acquiring() {
        let frame = GnssFrame {
            prn: 12,
            doppler_hz: -1234.0,
            code_phase_chips: 511.5,
            cn0_db_hz: 44.25,
            subframe: None,
            tow_seconds: None,
            week: None,
            words: Vec::new(),
        };
        let view = frame.present(&EventKind::named("gnss"));
        assert_eq!(
            view.labels(),
            ["Signal", "Doppler", "Code phase", "C/N0", "Subframe"]
        );
        assert_eq!(view.field("Subframe"), text(Some("acquiring")).as_ref());
        assert_eq!(
            view.field("C/N0"),
            number(Some(44.25), Unit::DbHz, 1).as_ref()
        );
        assert_eq!(
            view.field("Code phase"),
            number(Some(511.5), Unit::Chips, 2).as_ref()
        );
    }

    const PLACED_NODE: &str = r#"{"spreading_factor":11,"bandwidth_hz":250000.0,"coding_rate":"4/5","sync_word":43,"implicit_header":false,"low_data_rate":false,"inverted_iq":false,"integrity":"crc_ok","fec_corrected":0,"snr_db":1.0,"frequency_error_hz":0.0,"payload":"00","decoded":{"protocol":"meshtastic","to":4294967295,"from":161,"id":1,"hop_limit":3,"hop_start":3,"want_ack":false,"via_mqtt":false,"channel_hash":8,"next_hop":0,"relay_node":0,"encryption":"channel","content":{"type":"position","lat":47.5,"lon":8.25}}}"#;

    fn placed_node() -> LoraFrame {
        serde_json::from_str(PLACED_NODE).expect("lora fixture")
    }

    fn lora_with(content: MeshtasticContent) -> LoraFrame {
        let mut frame = placed_node();
        if let Some(LoraPayload::Meshtastic(packet)) = frame.decoded.as_mut() {
            packet.content = Some(content);
        }
        frame
    }

    #[test]
    fn a_lora_node_keeps_its_last_position_across_frames_without_one() {
        let catalog = sdrmm_wire::EventCatalog::new([lora_kind()]);
        let mut node = sdrmm_wire::DecoderEvent::of(&placed_node());
        let text = sdrmm_wire::DecoderEvent::of(&lora_with(MeshtasticContent::Text {
            text: "hi".to_owned(),
        }));
        assert!(catalog.merge(&mut node, &text));
        let view = catalog.present(&node);
        assert_eq!(view.entity.as_deref(), Some("!000000a1"));
        assert_eq!(
            view.place.map(|place| place.at),
            Some(sdrmm_wire::LatLon {
                lat: 47.5,
                lon: 8.25
            })
        );
    }

    #[test]
    fn a_skimmer_spot_shows_offset_speed_and_snr() {
        let view = CwSkimmerSpot {
            offset_hz: -120.0,
            text: "CQ DL1ABC".to_owned(),
            wpm: 24.0,
            snr_db: 11.5,
        }
        .present(&EventKind::named("cw_skimmer"));
        assert_eq!(view.labels(), ["Offset", "Speed", "SNR", "Text"]);
        assert_eq!(
            view.field("Offset"),
            number(Some(-120.0_f32), Unit::Hz, 0).as_ref()
        );
        assert_eq!(view.text, None);
    }

    #[test]
    fn a_call_lists_its_parties_and_timing() {
        let view = serde_json::from_value::<VoiceCall>(serde_json::json!({
            "id": 1, "node": "n", "started_at": "2026-01-01T00:00:00Z",
            "ended_at": "2026-01-01T00:00:04Z", "duration_ms": 4200, "device_set": 0,
            "channel": 0, "freq_hz": 438e6, "mode": "DMR", "slot": 1, "destination": 9,
            "group_call": false, "source": 2621, "encrypted": false, "emergency": true,
        }))
        .unwrap()
        .present(&EventKind::CALL);
        assert_eq!(view.field("Mode"), text_of("DMR").as_ref());
        assert_eq!(view.field("Destination"), text_of("radio 9").as_ref());
        assert_eq!(
            view.field("Duration"),
            number(Some(4.2), Unit::Seconds, 1).as_ref()
        );
        assert_eq!(
            view.field("Started"),
            Some(&FieldValue::Time("2026-01-01T00:00:00Z".to_owned()))
        );
        assert_eq!(view.field("Emergency"), only_if(true).as_ref());
        assert_eq!(view.field("Encrypted"), None);
    }

    #[test]
    fn a_transmission_lists_its_signal_and_decoder() {
        let mut heard: Transmission = serde_json::from_str(r#"{"id":1,"state":"completed","signal":{"modulation":"fm","confidence":0.9,"frequency_hz":145500000.0,"center_offset_hz":0.0,"bandwidth_hz":12500.0,"snr_db":20.0,"features":{"envelope_variation":0.1,"duty":1.0,"keying_depth_db":0.0,"spectral_asymmetry":0.0,"carrier_db":0.0,"spectral_flatness":0.2,"frequency_levels":0,"frequency_spread_hz":3000.0,"square_line_db":0.0,"quartic_line_db":0.0}},"start_sample":0,"end_sample":48000,"sample_rate_hz":48000.0,"duration_ms":1000,"audio":{"url":"/api/clips/1","media_type":"audio/wav"}}"#).expect("transmission fixture");
        let mut view = heard.present(&EventKind::TRANSMISSION);
        assert_eq!(view.field("State"), text_of("completed").as_ref());
        assert_eq!(
            view.field("Frequency"),
            number(Some(145_500_000.0), Unit::Hz, 0).as_ref()
        );
        assert_eq!(view.field("Confidence"), percent(0.9).as_ref());
        assert_eq!(
            view.field("Duration"),
            number(Some(1.0), Unit::Seconds, 2).as_ref()
        );
        assert_eq!(view.field("Decoder"), None);
        heard.decoder = Some("pocsag".to_owned());
        heard.decoder_confirmed = true;
        view = heard.present(&EventKind::TRANSMISSION);
        assert_eq!(
            view.field("Decoder"),
            text_of("POCSAG (confirmed)").as_ref()
        );
    }

    #[test]
    fn wspr_shows_timing_and_link_measurements() {
        let view = serde_json::from_value::<WsprSpot>(serde_json::json!({
            "text": "K1ABC FN42 37", "callsign": "K1ABC", "grid": "FN42", "power_dbm": 37,
            "snr_db": -21.0, "audio_hz": 1501.25, "time_offset_s": 0.75, "drift_hz": -0.2,
        }))
        .unwrap()
        .present(&EventKind::named("wspr"));
        assert_eq!(
            view.labels(),
            [
                "Callsign",
                "Grid",
                "Power",
                "SNR",
                "Audio",
                "Time offset",
                "Drift",
                "Message"
            ]
        );
        assert_eq!(view.field("Power"), number(Some(37), Unit::Dbm, 0).as_ref());
        assert_eq!(
            view.field("Time offset"),
            number(Some(0.75_f32), Unit::Seconds, 2).as_ref()
        );
        assert_eq!(view.field("Message"), text(Some("K1ABC FN42 37")).as_ref());
    }

    #[test]
    fn ft8_counts_hard_errors() {
        let view = WsjtMessage {
            text: "CQ DL1ABC JO62".to_owned(),
            snr_db: -12.0,
            audio_hz: 1234.5,
            time_offset_s: 0.1,
            hard_errors: 3,
        }
        .present(&EventKind::named("ft8"));
        assert_eq!(view.field("Hard errors"), count(Some(3)).as_ref());
        assert_eq!(
            view.field("SNR"),
            number(Some(-12.0_f32), Unit::Db, 0).as_ref()
        );
    }

    #[test]
    fn a_measured_width_snaps_to_the_nearest_lora_bandwidth() {
        assert_eq!(LoraBandwidth::nearest(118_000.0), LoraBandwidth::Khz125);
        assert_eq!(LoraBandwidth::nearest(240_000.0), LoraBandwidth::Khz250);
        assert_eq!(LoraBandwidth::nearest(1.0), LoraBandwidth::Khz7_8);
    }

    #[test]
    fn chips_per_symbol_name_the_spreading_factor() {
        assert_eq!(
            LoraSpreadingFactor::of_chips(128.0),
            LoraSpreadingFactor::Sf7
        );
        assert_eq!(
            LoraSpreadingFactor::of_chips(4_090.0),
            LoraSpreadingFactor::Sf12
        );
        assert_eq!(
            LoraSpreadingFactor::of_chips(32.0),
            LoraSpreadingFactor::All
        );
    }

    #[test]
    fn params_default_from_empty_settings_and_roundtrip() {
        let params: LoraParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params, LoraParams::default());
        assert_eq!(params.bandwidth.hz(), 125_000.0);
        assert_eq!(params.spreading_factor.factors(), 7..=12);
        let stated = LoraParams {
            bandwidth: LoraBandwidth::Khz62_5,
            spreading_factor: LoraSpreadingFactor::Sf8,
            iq: LoraIq::Both,
            protocol: LoraProtocol::Meshcore,
            implicit_header: Some(LoraImplicitHeader {
                length: 12,
                coding_rate: LoraCodingRate::Cr48,
                crc: true,
            }),
            keys: vec![LoraKey::MeshtasticChannel {
                name: "LongFast".to_owned(),
                psk: "AQ==".to_owned(),
            }],
        };
        let json = serde_json::to_value(&stated).unwrap();
        assert_eq!(json["bandwidth"], "khz62_5");
        assert_eq!(json["implicit_header"]["coding_rate"], "4/8");
        assert_eq!(json["keys"][0]["kind"], "meshtastic_channel");
        assert_eq!(serde_json::from_value::<LoraParams>(json).unwrap(), stated);
    }

    #[test]
    fn validation_bounds_keys_and_implicit_lengths() {
        let mut params = LoraParams::default();
        assert!(params.validate().is_ok());
        params.implicit_header = Some(LoraImplicitHeader {
            length: 0,
            coding_rate: LoraCodingRate::Cr45,
            crc: false,
        });
        assert!(params.validate().is_err());
        params.implicit_header = None;
        params.keys = vec![LoraKey::LorawanAppKey {
            app_key: "0".repeat(MAX_LORA_KEY_TEXT_LEN + 1),
        }];
        assert!(params.validate().is_err());
    }

    #[test]
    fn a_channel_name_is_bounded_by_the_name_limit_not_the_text_limit() {
        let named = |len: usize| LoraParams {
            keys: vec![LoraKey::MeshtasticChannel {
                name: "n".repeat(len),
                psk: "AQ==".to_owned(),
            }],
            ..LoraParams::default()
        };
        assert!(named(MAX_LORA_KEY_NAME_LEN).validate().is_ok());
        let refused = named(MAX_LORA_KEY_NAME_LEN + 1)
            .validate()
            .expect_err("a name over its own limit");
        assert!(refused.contains("name"), "{refused}");
        assert!(
            refused.contains(&MAX_LORA_KEY_NAME_LEN.to_string()),
            "{refused}"
        );
        assert!(named(MAX_LORA_KEY_TEXT_LEN).validate().is_err());
    }

    #[test]
    fn a_key_field_is_measured_in_characters() {
        let wide = LoraParams {
            keys: vec![LoraKey::MeshtasticChannel {
                name: "\u{8def}".repeat(MAX_LORA_KEY_NAME_LEN),
                psk: "AQ==".to_owned(),
            }],
            ..LoraParams::default()
        };
        assert!(wide.validate().is_ok());
    }

    #[test]
    fn coding_rates_map_to_parity_bits() {
        for rate in [
            LoraCodingRate::Cr45,
            LoraCodingRate::Cr46,
            LoraCodingRate::Cr47,
            LoraCodingRate::Cr48,
        ] {
            assert_eq!(
                LoraCodingRate::from_parity_bits(rate.parity_bits()),
                Some(rate)
            );
        }
        assert_eq!(LoraCodingRate::from_parity_bits(0), None);
    }

    fn frame(decoded: Option<LoraPayload>) -> LoraFrame {
        LoraFrame {
            spreading_factor: 11,
            bandwidth_hz: 250_000.0,
            coding_rate: LoraCodingRate::Cr45,
            sync_word: 0x2b,
            implicit_header: false,
            low_data_rate: false,
            inverted_iq: false,
            integrity: LoraIntegrity::CrcOk,
            fec_corrected: 0,
            snr_db: 3.0,
            frequency_error_hz: 120.0,
            payload: "00112233".to_owned(),
            decoded,
        }
    }

    #[test]
    fn a_meshtastic_position_names_its_node_and_place() {
        let packet = MeshtasticPacket {
            to: MeshtasticPacket::BROADCAST,
            from: 0xa1b2_c3d4,
            id: 7,
            hop_limit: 3,
            hop_start: 3,
            want_ack: false,
            via_mqtt: false,
            channel_hash: 8,
            next_hop: 0,
            relay_node: 0,
            encryption: MeshtasticEncryption::Channel,
            channel: Some("LongFast".to_owned()),
            port: Some(3),
            port_name: Some("POSITION_APP".to_owned()),
            request_id: None,
            reply_id: None,
            content: Some(MeshtasticContent::Position {
                lat: Some(47.5),
                lon: Some(8.25),
                altitude_m: None,
                time: None,
                satellites: None,
                ground_speed_kmh: None,
                precision_bits: None,
            }),
        };
        let frame = frame(Some(LoraPayload::Meshtastic(packet)));
        assert_eq!(frame.station().as_deref(), Some("!a1b2c3d4"));
        assert_eq!(frame.position(), Some((47.5, 8.25)));
        assert_eq!(
            frame.summary(),
            "SF11 250 kHz · !a1b2c3d4 → ^all · position 47.50000, 8.25000"
        );
    }

    #[test]
    fn a_raw_frame_reports_its_size_and_a_failed_check() {
        let mut raw = frame(None);
        raw.integrity = LoraIntegrity::CrcFailed;
        assert_eq!(raw.station(), None);
        assert_eq!(raw.summary(), "SF11 250 kHz · 4 bytes · CRC failed");
    }
}

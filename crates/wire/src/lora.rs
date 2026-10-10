use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub const MAX_LORA_KEYS: usize = 32;
pub const MAX_LORA_KEY_NAME_LEN: usize = 32;
pub const MAX_LORA_KEY_TEXT_LEN: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
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
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
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
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct LoraImplicitHeader {
    pub length: u8,
    #[serde(default)]
    pub coding_rate: LoraCodingRate,
    #[serde(default = "default_true")]
    pub crc: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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

impl LoraKey {
    fn texts(&self) -> Vec<&str> {
        match self {
            Self::MeshtasticChannel { name, psk } => vec![name, psk],
            Self::MeshcoreChannel { name, secret } => vec![name, secret],
            Self::LorawanSession {
                dev_addr,
                nwk_s_key,
                app_s_key,
            } => vec![dev_addr, nwk_s_key, app_s_key],
            Self::LorawanAppKey { app_key } => vec![app_key],
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct LoraParams {
    #[serde(default)]
    pub bandwidth: LoraBandwidth,
    #[serde(default)]
    pub spreading_factor: LoraSpreadingFactor,
    #[serde(default)]
    pub iq: LoraIq,
    #[serde(default)]
    pub protocol: LoraProtocol,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implicit_header: Option<LoraImplicitHeader>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<LoraKey>,
}

impl LoraParams {
    pub fn validate(&self) -> Result<(), String> {
        if self.keys.len() > MAX_LORA_KEYS {
            return Err(format!("at most {MAX_LORA_KEYS} LoRa keys"));
        }
        let too_long = self
            .keys
            .iter()
            .flat_map(LoraKey::texts)
            .any(|text| text.len() > MAX_LORA_KEY_TEXT_LEN.max(MAX_LORA_KEY_NAME_LEN));
        if too_long {
            return Err(format!(
                "LoRa key fields are at most {MAX_LORA_KEY_TEXT_LEN} characters"
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "protocol", rename_all = "snake_case")]
pub enum LoraPayload {
    Lorawan(LorawanFrame),
    Meshtastic(MeshtasticPacket),
    Meshcore(MeshcorePacket),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MeshtasticEncryption {
    Open,
    Channel,
    Pki,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MeshtasticMetric {
    pub name: String,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MeshtasticNeighbor {
    pub node: u32,
    pub snr_db: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MeshcoreRoute {
    TransportFlood,
    Flood,
    Direct,
    TransportDirect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MeshcoreNodeType {
    None,
    Chat,
    Repeater,
    Room,
    Sensor,
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
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
    pub fn summary(&self) -> String {
        let mut parts = vec![format!(
            "SF{} {}",
            self.spreading_factor,
            crate::units::hertz(self.bandwidth_hz)
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

#[cfg(test)]
mod tests {
    use super::*;

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

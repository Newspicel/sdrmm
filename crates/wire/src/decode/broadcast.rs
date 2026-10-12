use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{RadioClockStandard, channel::DabTransmissionMode};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RdsUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ps: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radiotext: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pty: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pty_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tp: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ta: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub music: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alt_freqs_hz: Vec<f64>,
    pub groups: u64,
    pub blocks: u64,
    pub block_errors: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BroadcastSystem {
    #[default]
    Dab,
    DabPlus,
    DvbS,
    DvbS2,
    DvbT,
    DvbT2,
    Drm30,
    DrmPlus,
}

impl BroadcastSystem {
    pub const ALL: [Self; 8] = [
        Self::Dab,
        Self::DabPlus,
        Self::DvbS,
        Self::DvbS2,
        Self::DvbT,
        Self::DvbT2,
        Self::Drm30,
        Self::DrmPlus,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dab => "DAB",
            Self::DabPlus => "DAB+",
            Self::DvbS => "DVB-S",
            Self::DvbS2 => "DVB-S2",
            Self::DvbT => "DVB-T",
            Self::DvbT2 => "DVB-T2",
            Self::Drm30 => "DRM30",
            Self::DrmPlus => "DRM+",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BroadcastServiceKind {
    #[default]
    Audio,
    Data,
    Video,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BroadcastService {
    pub id: u32,
    pub label: String,
    pub kind: BroadcastServiceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitrate_kbps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default)]
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BroadcastData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub label: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_id: Option<u32>,
    pub name: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SuperframeStatus {
    pub format: u8,
    pub sosf: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pilot: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trailer: Option<u8>,
    pub reference: u32,
    pub payload: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BroadcastStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_label: Option<String>,
    #[serde(default)]
    pub data_groups_ok: u32,
    #[serde(default)]
    pub data_groups_bad: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_error: Option<String>,
    #[serde(default)]
    pub video_frames_ok: u32,
    #[serde(default)]
    pub video_frames_bad: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_error: Option<String>,
    #[serde(default)]
    pub audio_frames_ok: u32,
    #[serde(default)]
    pub audio_frames_bad: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_error: Option<String>,
    pub system: BroadcastSystem,
    pub locked: bool,
    pub snr_db: f32,
    pub frequency_error_hz: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transmission_mode: Option<DabTransmissionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ensemble_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ensemble_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_rate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bit_error_rate: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitrate_kbps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default)]
    pub frames_ok: u32,
    #[serde(default)]
    pub frames_bad: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<BroadcastService>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superframe: Option<SuperframeStatus>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RadioClockFrame {
    pub standard: RadioClockStandard,
    pub datetime: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utc_offset_minutes: Option<i16>,
    #[serde(default)]
    pub dst: bool,
    #[serde(default)]
    pub leap_warning: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dut1_seconds: Option<f32>,
    pub symbols: String,
}

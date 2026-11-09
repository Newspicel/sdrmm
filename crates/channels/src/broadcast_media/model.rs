use sdrmm_wire::{EventContract, Hue, KindSpec, SettingChoice};
use serde::{Deserialize, Serialize};

use crate::dab::DabTransmissionMode;
use crate::datv::SuperframeStatus;
use sdrmm_wire::EventKind;

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::FieldValue;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::count;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::flag;
use sdrmm_wire::present::number;
use sdrmm_wire::present::text;
use sdrmm_wire::present::text_of;
use sdrmm_wire::present::trimmed;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BroadcastServiceKind {
    #[default]
    Audio,
    Data,
    Video,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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

/// The readout naming the system a decoder found, which its `Auto` system option shows.
pub(crate) const SYSTEM_READOUT: &str = "system";

/// The readout naming the DAB transmission mode found, which `Auto` shows.
pub(crate) const TRANSMISSION_MODE_READOUT: &str = "transmission_mode";

/// The readout naming the code rate found, which `Auto` shows.
pub(crate) const CODE_RATE_READOUT: &str = "code_rate";

/// The readout counting frames that failed their check.
pub(crate) const BAD_FRAMES_READOUT: &str = "bad_frames";

/// The readout naming the service played, which the service picker's `Auto` shows.
pub(crate) const SERVICE_READOUT: &str = "service";

/// The readouts a face carries: what an `Auto` option names, and the stream's bitrate.
pub(crate) const FACE_FIELDS: [&str; 5] = [
    SYSTEM_READOUT,
    TRANSMISSION_MODE_READOUT,
    CODE_RATE_READOUT,
    SERVICE_READOUT,
    "bitrate",
];

impl EventPayload for BroadcastStatus {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let system = self.system.label();
        let name = trimmed(self.label.as_deref()).unwrap_or_else(|| system.to_owned());
        let state = if self.locked { "locked" } else { "searching" };
        Presentation {
            fields: fields(&[
                (SYSTEM_READOUT, "System", text_of(system)),
                ("locked", "Locked", flag(Some(self.locked))),
                ("snr", "SNR", number(Some(self.snr_db), Unit::Db, 1)),
                (
                    "offset",
                    "Offset",
                    number(Some(self.frequency_error_hz), Unit::Hz, 0),
                ),
                (
                    TRANSMISSION_MODE_READOUT,
                    "Mode",
                    self.transmission_mode
                        .filter(|mode| *mode != DabTransmissionMode::Auto)
                        .and_then(|mode| text_of(mode.choice_label())),
                ),
                (SERVICE_READOUT, "Service", text(self.label.as_deref())),
                ("ensemble", "Ensemble", text(self.ensemble_label.as_deref())),
                (
                    "bitrate",
                    "Bitrate",
                    self.bitrate_kbps
                        .map(|kbps| FieldValue::Text(format!("{kbps} kbit/s"))),
                ),
                (
                    CODE_RATE_READOUT,
                    "Code rate",
                    text(self.code_rate.as_deref()),
                ),
                ("now", "Now", text(self.dynamic_label.as_deref())),
                ("frames", "Frames", count(Some(f64::from(self.frames_ok)))),
                (
                    BAD_FRAMES_READOUT,
                    "Bad frames",
                    count(Some(f64::from(self.frames_bad))),
                ),
            ]),
            verified: Some(self.locked),
            ..Presentation::of(format!("{name} {state} {:.1} dB", self.snr_db))
        }
    }
}

pub(crate) fn broadcast_kind() -> KindSpec {
    KindSpec::of::<BroadcastStatus>("Broadcast", "Services", Hue::Muted).build()
}

impl EventContract for BroadcastStatus {
    const KIND: EventKind = EventKind::named("broadcast");
}

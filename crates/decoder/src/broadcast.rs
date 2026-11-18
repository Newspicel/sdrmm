//! The status every broadcast decoder publishes, as its `broadcast` event and its node face:
//! lock and signal, the services found, and how the chosen one decodes.

use sdrmm_wire::present::{
    EventPayload, FieldValue, Presentation, Unit, count, fields, flag, number, text, text_of,
    trimmed,
};
use sdrmm_wire::{
    ChannelHealth, ChannelStatus, Control, DecoderEvent, EventContract, EventKind, Hue, KindSpec,
    NumberSpec, ParamChoice, ParamOption, SettingChoice, SettingField,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ChannelOutputs;

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
    /// The broadcast system found, by the id of the decoder's choice for it.
    #[serde(default)]
    pub system: String,
    /// What the face calls `system`; the id stands in while empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub system_label: String,
    pub locked: bool,
    pub snr_db: f32,
    pub frequency_error_hz: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_rate: Option<f64>,
    /// The transmission mode found, by the id of the decoder's choice for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transmission_mode: Option<String>,
    /// What the face calls `transmission_mode`; the id stands in while absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transmission_mode_label: Option<String>,
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
    /// What only one decoder measures, each under its own key.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The readout naming the system a decoder found, which its `Auto` system option shows.
pub const SYSTEM_READOUT: &str = "system";

/// The readout naming the transmission mode found, which `Auto` shows.
pub const TRANSMISSION_MODE_READOUT: &str = "transmission_mode";

/// The readout naming the code rate found, which `Auto` shows.
pub const CODE_RATE_READOUT: &str = "code_rate";

/// The readout counting frames that failed their check.
pub const BAD_FRAMES_READOUT: &str = "bad_frames";

/// The readout naming the service played, which the service picker's `Auto` shows.
pub const SERVICE_READOUT: &str = "service";

/// The readouts a face carries: what an `Auto` option names, and the stream's bitrate.
pub const FACE_FIELDS: [&str; 5] = [
    SYSTEM_READOUT,
    TRANSMISSION_MODE_READOUT,
    CODE_RATE_READOUT,
    SERVICE_READOUT,
    "bitrate",
];

/// The id an event carries for a decoder's `choice`.
fn id_of<T: SettingChoice>(choice: T) -> String {
    match serde_json::to_value(choice) {
        Ok(Value::String(id)) => id,
        _ => String::new(),
    }
}

impl BroadcastStatus {
    /// Names the system found by the decoder's choice for it.
    #[must_use]
    pub fn with_system<T: SettingChoice>(self, system: T) -> Self {
        Self {
            system: id_of(system),
            system_label: system.choice_label().to_owned(),
            ..self
        }
    }

    /// Names the transmission mode found by the decoder's choice for it.
    #[must_use]
    pub fn with_transmission_mode<T: SettingChoice>(self, mode: Option<T>) -> Self {
        Self {
            transmission_mode: mode.map(id_of),
            transmission_mode_label: mode.map(|mode| mode.choice_label().to_owned()),
            ..self
        }
    }

    fn system_name(&self) -> &str {
        if self.system_label.is_empty() {
            &self.system
        } else {
            &self.system_label
        }
    }
}

impl EventPayload for BroadcastStatus {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let system = self.system_name();
        let name = trimmed(self.label.as_deref()).unwrap_or_else(|| system.to_owned());
        let state = if self.locked { "locked" } else { "searching" };
        let mode = self
            .transmission_mode_label
            .as_deref()
            .or(self.transmission_mode.as_deref());
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
                (TRANSMISSION_MODE_READOUT, "Mode", text(mode)),
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

impl EventContract for BroadcastStatus {
    const KIND: EventKind = EventKind::named("broadcast");
}

/// The `broadcast` event kind each broadcast decoder declares.
#[must_use]
pub fn broadcast_kind() -> KindSpec {
    KindSpec::of::<BroadcastStatus>("Broadcast", "Services", Hue::Muted).build()
}

/// A broadcast decoder's status as both its `broadcast` event and its node face: the readouts,
/// and the services it found offered for the `service_key` setting. The face keeps how the
/// stream decodes ([`FACE_FIELDS`], bad frames once there are any); what it decoded, such as
/// the ensemble or the text now playing, travels on the event alone.
pub fn publish_status(out: &mut ChannelOutputs, status: &BroadcastStatus, service_key: &str) {
    out.events.push(DecoderEvent::of(status));
    let mut readouts = status.present(&BroadcastStatus::KIND).fields;
    readouts.retain(|field| {
        FACE_FIELDS.contains(&field.key.as_str())
            || (field.key == BAD_FRAMES_READOUT && status.frames_bad > 0)
    });
    out.status = Some(ChannelStatus {
        health: health(status),
        readouts,
        choices: vec![ParamChoice {
            name: service_key.to_owned(),
            options: service_options(&status.services),
        }],
    });
}

/// Lock always; SNR and frequency error only while locked, as a searching receiver has
/// measured neither.
fn health(status: &BroadcastStatus) -> ChannelHealth {
    let health = ChannelHealth::default().lock(status.locked);
    if status.locked {
        health
            .snr(status.snr_db)
            .freq_error(f64::from(status.frequency_error_hz))
    } else {
        health
    }
}

/// The newest status a broadcast decoder published into `out`.
#[must_use]
pub fn latest_status(out: &ChannelOutputs) -> Option<BroadcastStatus> {
    out.events
        .iter()
        .rev()
        .find_map(DecoderEvent::read::<BroadcastStatus>)
}

/// The services found, led by `Auto` naming the one it chose; none while nothing was found,
/// so the picker keeps its stand-in.
fn service_options(services: &[BroadcastService]) -> Vec<ParamOption> {
    if services.is_empty() {
        return Vec::new();
    }
    let auto = ParamOption::new(Value::Null, "Auto").status(SERVICE_READOUT);
    std::iter::once(auto)
        .chain(services.iter().map(service_option))
        .collect()
}

fn service_option(service: &BroadcastService) -> ParamOption {
    let label = service.label.trim();
    ParamOption {
        value: service.id.into(),
        label: if label.is_empty() {
            service.id.to_string()
        } else {
            label.to_owned()
        },
        title: None,
        status: None,
    }
}

/// The service a broadcast decoder plays, picked from the services its status lists.
#[must_use]
pub fn service_field(key: &str, max: f64) -> SettingField {
    SettingField::new(
        key,
        "Service",
        Control::live(NumberSpec::new(0.0, max, 1.0).optional("Auto")),
    )
    .title(
        "Select a discovered audio, video or data service; Auto chooses the first playable service",
    )
}

#[cfg(test)]
mod tests;

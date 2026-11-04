use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    EventKind,
    audio::NoiseBlankerSettings,
    network::NetworkExportStatus,
    state::{AudioRecordingStatus, RecordingStatus},
};

mod choices;
mod form;
mod if_filter;
mod params;
mod status;

pub use choices::{ParamChoice, ParamOption, SettingChoice};
pub use form::{Condition, Control, LanePick, NumberSpec, PresetOption, SettingField, lookup};
pub use if_filter::{
    DEFAULT_IF_NOTCH_WIDTH_HZ, IfFilter, IfNotch, MAX_IF_EDGE_HZ, MAX_IF_NOTCH_WIDTH_HZ,
    MAX_IF_NOTCHES, MIN_IF_NOTCH_WIDTH_HZ, MIN_IF_PASSBAND_HZ,
};
pub use params::{ChannelParams, DecoderSettings};
pub use status::{ChannelHealth, ChannelStatus};

crate::open_name::open_name!(
    /// The palette group a decoder sits in.
    DecoderFamily {
        ANALOG_VOICE = "analog_voice",
        DIGITAL_VOICE = "digital_voice",
        AVIATION = "aviation",
        MARINE = "marine",
        AMATEUR = "amateur",
        PAGING = "paging",
        VIDEO = "video",
        BROADCAST = "broadcast",
        WEATHER = "weather",
        UTILITY = "utility",
    }
);

impl Default for DecoderFamily {
    fn default() -> Self {
        Self::UTILITY
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ChannelDescriptor {
    pub type_id: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub family: DecoderFamily,
    pub bandwidth_hz: f64,
    pub input_rate_hz: f64,
    #[serde(default = "default_has_audio")]
    pub has_audio: bool,
    #[serde(default)]
    pub emits: Vec<EventKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calls: Option<CallGate>,
    #[serde(default)]
    pub has_video: bool,
    #[serde(default)]
    pub can_transmit: bool,
    #[serde(default)]
    pub needs_position: bool,
    /// Signal ID can name this decoder's signal: it has `signatures`.
    #[serde(default)]
    pub identifiable: bool,
    /// How this decoder's signal shows on the air, for Signal ID.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signatures: Vec<crate::decode::SignalSignature>,
    /// How Signal ID proves a signal is this decoder's by running it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trial: Option<crate::decode::SignalTrial>,
    /// What a fresh channel of this type starts on, so a node can show and edit its settings
    /// before any radio is open to carry it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<ChannelSettings>,
    /// The form a channel node renders for this decoder's settings, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<SettingField>,
    #[serde(default)]
    pub click: ClickProfile,
    /// The demodulator already levels its audio, so an added AGC stage is redundant.
    #[serde(default)]
    pub levels_audio: bool,
    /// One of the plain analog voice modes a channel cycles through when retyped by hotkey.
    #[serde(default)]
    pub analog_voice: bool,
    /// A trunk system can run on this decoder.
    #[serde(default)]
    pub trunks: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<crate::patch::PortSpec>,
}

/// The shape of the impulses a click remover looks for in this decoder's audio.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClickProfile {
    #[default]
    Discriminator,
    Detector,
    Vocoder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CallGate {
    Frames,
    Squelch,
}

impl ChannelDescriptor {
    #[must_use]
    pub fn has_events(&self) -> bool {
        !self.emits.is_empty() || self.calls.is_some()
    }
}

fn default_has_audio() -> bool {
    true
}

impl Default for ChannelDescriptor {
    fn default() -> Self {
        Self {
            type_id: String::new(),
            name: String::new(),
            summary: String::new(),
            family: DecoderFamily::default(),
            bandwidth_hz: 0.0,
            input_rate_hz: 0.0,
            has_audio: default_has_audio(),
            emits: Vec::new(),
            calls: None,
            has_video: false,
            can_transmit: false,
            needs_position: false,
            identifiable: false,
            signatures: Vec::new(),
            trial: None,
            defaults: None,
            settings: Vec::new(),
            click: ClickProfile::default(),
            levels_audio: false,
            analog_voice: false,
            trunks: false,
            ports: Vec::new(),
        }
    }
}

pub const MIN_SQUELCH_AUTO_MARGIN_DB: f32 = 2.0;
pub const MAX_SQUELCH_AUTO_MARGIN_DB: f32 = 40.0;
pub const DEFAULT_SQUELCH_AUTO_MARGIN_DB: f32 = 8.0;

/// How a channel gates what it decodes: not at all, above a level the operator set, or a margin
/// above the noise floor it measures for itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Squelch {
    #[default]
    Off,
    Manual {
        level_db: f32,
    },
    Auto {
        margin_db: f32,
    },
}

impl Squelch {
    #[must_use]
    pub fn is_off(&self) -> bool {
        matches!(self, Self::Off)
    }

    #[must_use]
    pub fn manual_level_db(&self) -> Option<f32> {
        match self {
            Self::Manual { level_db } => Some(*level_db),
            _ => None,
        }
    }

    #[must_use]
    pub fn auto_margin_db(&self) -> Option<f32> {
        match self {
            Self::Auto { margin_db } => Some(*margin_db),
            _ => None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match *self {
            Self::Off => Ok(()),
            Self::Manual { level_db } if level_db.is_finite() => Ok(()),
            Self::Manual { level_db } => {
                Err(format!("squelch level must be finite, got {level_db}"))
            }
            Self::Auto { margin_db }
                if margin_db.is_finite()
                    && (MIN_SQUELCH_AUTO_MARGIN_DB..=MAX_SQUELCH_AUTO_MARGIN_DB)
                        .contains(&margin_db) =>
            {
                Ok(())
            }
            Self::Auto { margin_db } => Err(format!(
                "squelch margin must be in {MIN_SQUELCH_AUTO_MARGIN_DB}..={MAX_SQUELCH_AUTO_MARGIN_DB} dB above the noise floor, got {margin_db}"
            )),
        }
    }

    #[must_use]
    pub fn from_levels(level_db: Option<f32>, margin_db: Option<f32>) -> Self {
        match (level_db, margin_db) {
            (_, Some(margin_db)) => Self::Auto { margin_db },
            (Some(level_db), None) => Self::Manual { level_db },
            (None, None) => Self::Off,
        }
    }
}

pub const DEFAULT_FREQUENCY_HZ: f64 = 100_000_000.0;

#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct ChannelSettings {
    /// The frequency the decoder listens on, whatever any radio happens to be tuned to. A radio
    /// that cannot reach it simply does not carry this channel.
    #[schema(required = false)]
    pub frequency_hz: f64,
    #[serde(default)]
    pub squelch: Squelch,
    pub params: ChannelParams,
    #[serde(default)]
    pub blanker: NoiseBlankerSettings,
    #[serde(default)]
    pub if_filter: IfFilter,
}

impl ChannelSettings {
    #[must_use]
    pub fn new(frequency_hz: f64, params: ChannelParams) -> Self {
        Self {
            frequency_hz,
            squelch: Squelch::Off,
            params,
            blanker: NoiseBlankerSettings::default(),
            if_filter: IfFilter::default(),
        }
    }

    /// Every setting checked against the decoder's form, with the squelch and IF filter.
    pub fn check_limits(&self, form: &[SettingField]) -> Result<(), String> {
        self.squelch.validate()?;
        self.if_filter.validate()?;
        form::check(self.params.type_id(), &self.params.settings, form)
    }
}

impl<'de> Deserialize<'de> for ChannelSettings {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Stated {
            frequency_hz: Option<f64>,
            #[serde(default)]
            squelch: Option<Squelch>,
            #[serde(default)]
            squelch_db: Option<f32>,
            #[serde(default)]
            squelch_auto_db: Option<f32>,
            params: ChannelParams,
            #[serde(default)]
            blanker: Option<NoiseBlankerSettings>,
            #[serde(default)]
            if_filter: IfFilter,
            #[serde(default)]
            audio: Option<LegacyAudio>,
        }
        #[derive(Deserialize)]
        struct LegacyAudio {
            #[serde(default)]
            blanker: NoiseBlankerSettings,
        }
        let stated = Stated::deserialize(deserializer)?;
        let blanker = stated
            .blanker
            .or_else(|| stated.audio.map(|audio| audio.blanker))
            .unwrap_or_default();
        Ok(Self {
            frequency_hz: stated.frequency_hz.unwrap_or(DEFAULT_FREQUENCY_HZ),
            squelch: stated
                .squelch
                .unwrap_or_else(|| Squelch::from_levels(stated.squelch_db, stated.squelch_auto_db)),
            params: stated.params,
            blanker,
            if_filter: stated.if_filter,
        })
    }
}

pub const RETIRED_CHANNEL_TYPES: &[&str] = &["subghz"];

#[must_use]
pub fn retired_channel_type(type_id: &str) -> bool {
    RETIRED_CHANNEL_TYPES.contains(&type_id)
}

pub(crate) fn states_retired_params(settings: &serde_json::Value) -> bool {
    settings
        .get("params")
        .and_then(|params| params.get("type"))
        .and_then(serde_json::Value::as_str)
        .is_some_and(retired_channel_type)
}

pub(crate) fn current_channel_settings<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<ChannelSettings>, D::Error> {
    Vec::<serde_json::Value>::deserialize(deserializer)?
        .into_iter()
        .filter(|settings| !states_retired_params(settings))
        .map(|settings| ChannelSettings::deserialize(settings).map_err(serde::de::Error::custom))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum BandMiss {
    TooWide {
        needs_hz: f64,
        top_rate_hz: Option<f64>,
    },
    OffTuner,
    Crowded,
    TunedAway,
}

/// Lowest and highest occupied offset from the channel frequency, as its decoder lays it out.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ChannelBand {
    pub low_hz: f64,
    pub high_hz: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ChannelInfo {
    pub id: u32,
    #[serde(default)]
    pub stream: u32,
    /// The patch node this decoder was opened for, so a workspace finds its own decoder again
    /// rather than the next one of the same kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    pub settings: ChannelSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band: Option<ChannelBand>,
    /// Why the radio carrying this decoder cannot hear it, so the channel is alive and set up but
    /// silent until the radio comes back over it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_of_band: Option<BandMiss>,
    #[serde(default)]
    pub rejected: u32,
    #[serde(default)]
    pub skipped: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_recordings: Vec<AudioRecordingStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseband_recording: Option<RecordingStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_export: Option<NetworkExportStatus>,
}

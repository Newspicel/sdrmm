use serde::Serialize;

use crate::{
    array::{
        DEFAULT_CAL_BANDWIDTH_HZ, LIGHT_SPEED_M_S, MAX_ARRAY_EXTENT_M, MAX_ARRAY_GAIN_DB,
        MAX_ARRAY_LANES, MAX_CAL_BANDWIDTH_HZ, MAX_CAL_OFFSET_HZ, MIN_ARRAY_GAIN_DB,
        MIN_ARRAY_LANES, MIN_CAL_BANDWIDTH_HZ,
    },
    fusion::{
        DEFAULT_FUSION_HALF_LIFE_S, FUSION_FIXED_HALF_LIFE_S, FUSION_MOVING_HALF_LIFE_S,
        MAX_FUSION_EMITTERS, MAX_FUSION_HALF_LIFE_S, MIN_FUSION_HALF_LIFE_S,
    },
    processor::DEFAULT_BAND_HZ,
    radar::{SURFACE_DB_MAX, SURFACE_DB_MIN},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Bounds<T> {
    pub min: T,
    pub max: T,
    #[serde(skip_serializing_if = "includes_min")]
    pub above: bool,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if takes a reference"
)]
const fn includes_min(above: &bool) -> bool {
    !*above
}

impl<T: Copy + PartialOrd> Bounds<T> {
    #[must_use]
    pub const fn new(min: T, max: T) -> Self {
        Self {
            min,
            max,
            above: false,
        }
    }

    #[must_use]
    pub const fn above(min: T, max: T) -> Self {
        Self {
            min,
            max,
            above: true,
        }
    }

    #[must_use]
    pub fn contains(&self, value: T) -> bool {
        let low = if self.above {
            self.min < value
        } else {
            self.min <= value
        };
        low && value <= self.max
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Limits {
    pub light_speed_m_s: f64,
    pub band_seed_hz: f64,
    pub spectrum_signal_margin_db: f32,
    pub array: ArrayLimits,
    pub fusion: FusionLimits,
    /// Levels a range-Doppler surface spans.
    pub range_doppler_db: Bounds<f32>,
    pub hunt: HuntLimits,
    pub presence: PresenceLimits,
    pub rotator: RotatorLimits,
    pub schedule: ScheduleLimits,
    pub patch: PatchLimits,
    pub scan: ScanLimits,
    pub name_len: usize,
    pub note_len: usize,
    pub filter: FilterLimits,
    pub channel_frequency_seed_hz: f64,
    pub audio: AudioLimits,
    pub if_filter: IfFilterLimits,
    pub survey: SurveyLimits,
    pub decoder_log_rows: u32,
    pub workspace_state_version: u32,
    pub voice_chat: VoiceChatLimits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct PatchLimits {
    pub nodes: usize,
    pub wires: usize,
    pub streams: u32,
    pub channels_per_radio: usize,
    pub monitors_per_radio: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ScanLimits {
    pub targets: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct FilterLimits {
    pub kinds: usize,
    pub ids: usize,
    pub duration_ms: u32,
    pub text_len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct AudioLimits {
    pub rate_hz: u32,
    pub notches: usize,
    pub blanker_threshold: Bounds<f32>,
    pub blanker_threshold_seed: f32,
    pub click_threshold: Bounds<f32>,
    pub click_threshold_seed: f32,
    pub squelch_auto_margin_db: Bounds<f32>,
    pub squelch_auto_margin_seed_db: f32,
    pub tone_hz: Bounds<f64>,
    pub notch_width_hz: Bounds<f64>,
    pub notch_freq_seed_hz: f64,
    pub notch_width_seed_hz: f64,
    pub denoise_strength_seed: f32,
    pub filter_low_seed_hz: f64,
    pub filter_high_seed_hz: f64,
    pub peak_gain_db: Bounds<f64>,
    pub peak_freq_seed_hz: f64,
    pub peak_width_seed_hz: f64,
    pub peak_gain_seed_db: f64,
}

pub const AUDIO_LIMITS: AudioLimits = AudioLimits {
    rate_hz: crate::audio::AUDIO_RATE_HZ,
    notches: crate::audio::MAX_AUDIO_NOTCHES,
    blanker_threshold: Bounds::new(
        crate::audio::MIN_BLANKER_THRESHOLD,
        crate::audio::MAX_BLANKER_THRESHOLD,
    ),
    blanker_threshold_seed: crate::audio::DEFAULT_BLANKER_THRESHOLD,
    click_threshold: Bounds::new(
        crate::audio::MIN_CLICK_THRESHOLD,
        crate::audio::MAX_CLICK_THRESHOLD,
    ),
    click_threshold_seed: crate::audio::DEFAULT_CLICK_THRESHOLD,
    squelch_auto_margin_db: Bounds::new(
        crate::channel::MIN_SQUELCH_AUTO_MARGIN_DB,
        crate::channel::MAX_SQUELCH_AUTO_MARGIN_DB,
    ),
    squelch_auto_margin_seed_db: crate::channel::DEFAULT_SQUELCH_AUTO_MARGIN_DB,
    tone_hz: Bounds::new(
        crate::audio::MIN_AUDIO_TONE_HZ,
        crate::audio::MAX_AUDIO_TONE_HZ,
    ),
    notch_width_hz: Bounds::new(
        crate::audio::MIN_NOTCH_WIDTH_HZ,
        crate::audio::MAX_NOTCH_WIDTH_HZ,
    ),
    notch_freq_seed_hz: crate::audio::DEFAULT_NOTCH_FREQ_HZ,
    notch_width_seed_hz: crate::audio::DEFAULT_NOTCH_WIDTH_HZ,
    denoise_strength_seed: crate::audio::DEFAULT_DENOISE_STRENGTH,
    filter_low_seed_hz: crate::audio::DEFAULT_FILTER_LOW_HZ,
    filter_high_seed_hz: crate::audio::DEFAULT_FILTER_HIGH_HZ,
    peak_gain_db: Bounds::new(
        crate::audio::MIN_PEAK_GAIN_DB,
        crate::audio::MAX_PEAK_GAIN_DB,
    ),
    peak_freq_seed_hz: crate::audio::DEFAULT_PEAK_FREQ_HZ,
    peak_width_seed_hz: crate::audio::DEFAULT_PEAK_WIDTH_HZ,
    peak_gain_seed_db: crate::audio::DEFAULT_PEAK_GAIN_DB,
};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct IfFilterLimits {
    pub notches: usize,
    pub edge_hz: Bounds<f64>,
    pub passband_hz: f64,
    pub notch_width_hz: Bounds<f64>,
    pub notch_width_seed_hz: f64,
}

pub const IF_FILTER_LIMITS: IfFilterLimits = IfFilterLimits {
    notches: crate::channel::MAX_IF_NOTCHES,
    edge_hz: Bounds::new(
        -crate::channel::MAX_IF_EDGE_HZ,
        crate::channel::MAX_IF_EDGE_HZ,
    ),
    passband_hz: crate::channel::MIN_IF_PASSBAND_HZ,
    notch_width_hz: Bounds::new(
        crate::channel::MIN_IF_NOTCH_WIDTH_HZ,
        crate::channel::MAX_IF_NOTCH_WIDTH_HZ,
    ),
    notch_width_seed_hz: crate::channel::DEFAULT_IF_NOTCH_WIDTH_HZ,
};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SurveyLimits {
    pub cells: usize,
    pub cell_m: f64,
    pub metres_per_degree_lon: f64,
    pub metres_per_degree_lat: f64,
    pub min_cos_lat: f64,
}

pub const SURVEY_LIMITS: SurveyLimits = SurveyLimits {
    cells: crate::survey::MAX_SURVEY_CELLS,
    cell_m: crate::survey::SURVEY_CELL_M,
    metres_per_degree_lon: crate::survey::METRES_PER_DEGREE_LON,
    metres_per_degree_lat: crate::survey::METRES_PER_DEGREE_LAT,
    min_cos_lat: crate::survey::MIN_COS_LAT,
};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ScheduleLimits {
    pub windows: usize,
    pub minutes_per_day: u16,
    pub zone_len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct RotatorLimits {
    pub tolerance_deg: Bounds<f64>,
    pub tolerance_seed_deg: f64,
    pub azimuth_deg: Bounds<f64>,
    pub heading_deg: Bounds<f64>,
    pub level_elevation_deg: f64,
    pub flip_elevation_deg: f64,
    pub gs232_baud: u32,
    pub winegard_baud: u32,
    pub rotctld_address: &'static str,
}

pub const ROTATOR_LIMITS: RotatorLimits = RotatorLimits {
    tolerance_deg: Bounds::new(
        crate::rotator::MIN_ROTATOR_TOLERANCE_DEG,
        crate::rotator::MAX_ROTATOR_TOLERANCE_DEG,
    ),
    tolerance_seed_deg: crate::rotator::DEFAULT_ROTATOR_TOLERANCE_DEG,
    azimuth_deg: Bounds::new(
        crate::rotator::MIN_ROTATOR_AZIMUTH_DEG,
        crate::rotator::MAX_ROTATOR_AZIMUTH_DEG,
    ),
    heading_deg: Bounds::new(0.0, crate::rotator::MAX_ROTATOR_HEADING_DEG),
    level_elevation_deg: crate::rotator::LEVEL_ELEVATION_DEG,
    flip_elevation_deg: crate::rotator::FLIP_ELEVATION_DEG,
    gs232_baud: crate::rotator::DEFAULT_GS232_BAUD,
    winegard_baud: crate::rotator::DEFAULT_WINEGARD_BAUD,
    rotctld_address: crate::rotator::DEFAULT_ROTCTLD_ADDRESS,
};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct VoiceChatLimits {
    pub threshold_db: Bounds<f64>,
    pub threshold_seed_db: f64,
    pub hang_ms: Bounds<u32>,
    pub hang_seed_ms: u32,
    pub volume: Bounds<f64>,
    pub volume_seed: f64,
    pub account_len: usize,
    pub discord_nickname_len: usize,
    pub channel_len: usize,
    pub allowed: usize,
    pub allowed_len: usize,
}

pub const VOICE_CHAT_LIMITS: VoiceChatLimits = VoiceChatLimits {
    threshold_db: Bounds::new(
        crate::voice_chat::MIN_VOX_THRESHOLD_DB,
        crate::voice_chat::MAX_VOX_THRESHOLD_DB,
    ),
    threshold_seed_db: crate::voice_chat::DEFAULT_VOX_THRESHOLD_DB,
    hang_ms: Bounds::new(0, crate::voice_chat::MAX_VOX_HANG_MS),
    hang_seed_ms: crate::voice_chat::DEFAULT_VOX_HANG_MS,
    volume: Bounds::new(
        crate::voice_chat::MIN_VOICE_VOLUME,
        crate::voice_chat::MAX_VOICE_VOLUME,
    ),
    volume_seed: crate::voice_chat::DEFAULT_VOICE_VOLUME,
    account_len: crate::voice_chat::MAX_CHAT_ACCOUNT_LEN,
    discord_nickname_len: crate::voice_chat::MAX_DISCORD_NICKNAME_LEN,
    channel_len: crate::voice_chat::MAX_CHAT_CHANNEL_LEN,
    allowed: crate::voice_chat::MAX_CHAT_ALLOWED,
    allowed_len: crate::voice_chat::MAX_CHAT_ALLOWED_LEN,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct PresenceLimits {
    pub name_len: usize,
    pub pointer_rate_hz: u32,
}

pub const PRESENCE_LIMITS: PresenceLimits = PresenceLimits {
    name_len: crate::presence::MAX_PEER_NAME_LEN,
    pointer_rate_hz: crate::presence::POINTER_RATE_HZ,
};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ArrayLimits {
    pub lanes: Bounds<u32>,
    pub extent_m: f64,
    pub gain_db: Bounds<f64>,
    pub cal_offset_hz: Bounds<f64>,
    pub cal_bandwidth_hz: Bounds<f64>,
    pub cal_bandwidth_seed_hz: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct FusionLimits {
    pub fixed_half_life_s: u32,
    pub moving_half_life_s: u32,
    pub half_life_s: Bounds<u32>,
    pub half_life_seed_s: u32,
    pub extent_km: Bounds<f64>,
    pub probe_km: Bounds<f64>,
    pub min_confidence: Bounds<f32>,
    pub emitters: Bounds<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct BandLimits {
    pub offset_hz: Bounds<f64>,
    pub bandwidth_hz: Bounds<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct HuntLimits {
    pub beamwidth_deg: Bounds<f64>,
    pub front_back_db: Bounds<f64>,
    pub min_span_deg: Bounds<f64>,
    pub min_contrast_db: Bounds<f32>,
    pub mount_offset_deg: Bounds<f64>,
}

pub const ARRAY_LIMITS: ArrayLimits = ArrayLimits {
    lanes: Bounds::new(MIN_ARRAY_LANES, MAX_ARRAY_LANES),
    extent_m: MAX_ARRAY_EXTENT_M,
    gain_db: Bounds::new(MIN_ARRAY_GAIN_DB, MAX_ARRAY_GAIN_DB),
    cal_offset_hz: Bounds::new(-MAX_CAL_OFFSET_HZ, MAX_CAL_OFFSET_HZ),
    cal_bandwidth_hz: Bounds::new(MIN_CAL_BANDWIDTH_HZ, MAX_CAL_BANDWIDTH_HZ),
    cal_bandwidth_seed_hz: DEFAULT_CAL_BANDWIDTH_HZ,
};

pub const FUSION_LIMITS: FusionLimits = FusionLimits {
    fixed_half_life_s: FUSION_FIXED_HALF_LIFE_S,
    moving_half_life_s: FUSION_MOVING_HALF_LIFE_S,
    half_life_s: Bounds::new(MIN_FUSION_HALF_LIFE_S, MAX_FUSION_HALF_LIFE_S),
    half_life_seed_s: DEFAULT_FUSION_HALF_LIFE_S,
    extent_km: Bounds::new(1.0, 100.0),
    probe_km: Bounds::new(0.5, 50.0),
    min_confidence: Bounds::new(0.0, 1.0),
    emitters: Bounds::new(1, MAX_FUSION_EMITTERS),
};

pub const HUNT_LIMITS: HuntLimits = HuntLimits {
    beamwidth_deg: Bounds::new(10.0, 180.0),
    front_back_db: Bounds::new(0.0, 40.0),
    min_span_deg: Bounds::new(60.0, 720.0),
    min_contrast_db: Bounds::new(1.0, 40.0),
    mount_offset_deg: Bounds::new(-180.0, 180.0),
};

pub const LIMITS: Limits = Limits {
    light_speed_m_s: LIGHT_SPEED_M_S,
    band_seed_hz: DEFAULT_BAND_HZ,
    spectrum_signal_margin_db: crate::frame::SPECTRUM_SIGNAL_MARGIN_DB,
    array: ARRAY_LIMITS,
    fusion: FUSION_LIMITS,
    range_doppler_db: Bounds::new(SURFACE_DB_MIN, SURFACE_DB_MAX),
    hunt: HUNT_LIMITS,
    presence: PRESENCE_LIMITS,
    rotator: ROTATOR_LIMITS,
    schedule: ScheduleLimits {
        windows: crate::schedule::MAX_SCHEDULE_WINDOWS,
        minutes_per_day: crate::schedule::MINUTES_PER_DAY,
        zone_len: crate::schedule::MAX_TIME_ZONE_LEN,
    },
    patch: PatchLimits {
        nodes: crate::patch::MAX_NODES,
        wires: crate::patch::MAX_EDGES,
        streams: crate::patch::MAX_STREAMS,
        channels_per_radio: crate::patch::MAX_CHANNELS_PER_RADIO,
        monitors_per_radio: crate::patch::MAX_MONITORS_PER_RADIO,
    },
    scan: ScanLimits {
        targets: crate::scan::MAX_SCAN_TARGETS,
    },
    name_len: crate::workspace::MAX_NAME_LEN,
    note_len: crate::note::MAX_NOTE_LEN,
    filter: FilterLimits {
        kinds: crate::filter::MAX_FILTER_KINDS,
        ids: crate::filter::MAX_FILTER_IDS,
        duration_ms: crate::filter::MAX_FILTER_DURATION_MS,
        text_len: crate::filter::MAX_FILTER_TEXT_LEN,
    },
    channel_frequency_seed_hz: crate::channel::DEFAULT_FREQUENCY_HZ,
    audio: AUDIO_LIMITS,
    if_filter: IF_FILTER_LIMITS,
    survey: SURVEY_LIMITS,
    decoder_log_rows: crate::rest::MAX_DECODER_LOG_ROWS,
    workspace_state_version: crate::workspace_state::WORKSPACE_STATE_VERSION,
    voice_chat: VOICE_CHAT_LIMITS,
};

pub fn generated() -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&LIMITS)
}

#[cfg(test)]
mod tests;

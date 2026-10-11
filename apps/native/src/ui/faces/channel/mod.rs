macro_rules! get {
    ($variant:ident, $p:ident => $e:expr) => {
        |params: &ChannelParams| match params {
            ChannelParams::$variant($p) => Some($e),
            _ => None,
        }
    };
}

macro_rules! set {
    ($variant:ident, $p:ident, $v:ident => $e:expr) => {
        |params: &mut ChannelParams, $v| {
            if let ChannelParams::$variant($p) = params {
                $e;
            }
        }
    };
}

pub mod alert;
pub mod band_miss;
pub mod binding;
pub mod controls;
pub mod edit;
mod face;
pub mod health;
pub mod lora;
mod lora_view;
pub mod modes;
mod more_modes;
pub mod picker;
pub mod settings;
pub mod swap;
pub mod tables;

use sdrmm_wire::{
    channel::{BandMiss, Squelch},
    device::Capabilities,
    state::DeviceSet,
};

use self::{band_miss::band_miss_said, binding::ChannelBinding, settings::DEFAULT_SQUELCH_DB};
use crate::store::Store;

pub use face::face;
pub use swap::cycle_analog;

pub const SQUELCH_STEP_DB: f32 = 2.0;

pub fn adjust_squelch(store: Store, node: &str, delta_db: f32) {
    store.edit_channel(node, move |settings| {
        settings.squelch = settings::nudged_squelch(&settings.squelch, delta_db);
    });
}

pub fn toggle_squelch(store: Store, node: &str) {
    store.edit_channel(node, |settings| {
        settings.squelch = if settings.squelch.is_off() {
            Squelch::Manual {
                level_db: DEFAULT_SQUELCH_DB,
            }
        } else {
            Squelch::Off
        };
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hotkey {
    CycleMode(i32),
    Squelch(i8),
    ToggleSquelch,
}

#[must_use]
pub fn hotkey_of(key: &str) -> Option<Hotkey> {
    Some(match key {
        "m" => Hotkey::CycleMode(1),
        "M" => Hotkey::CycleMode(-1),
        "-" => Hotkey::Squelch(-1),
        "=" | "+" => Hotkey::Squelch(1),
        "s" => Hotkey::ToggleSquelch,
        _ => return None,
    })
}

pub fn run_hotkey(store: Store, node: &str, key: Hotkey) {
    match key {
        Hotkey::CycleMode(direction) => {
            cycle_analog(store, node, direction);
        }
        Hotkey::Squelch(sign) => adjust_squelch(store, node, SQUELCH_STEP_DB * f32::from(sign)),
        Hotkey::ToggleSquelch => toggle_squelch(store, node),
    }
}

pub const ANY_FREQUENCY: (f64, f64) = (0.0, 6e9);

#[must_use]
pub fn tuning_range(capabilities: &Capabilities) -> (f64, f64) {
    let ranges = &capabilities.freq_ranges;
    if ranges.is_empty() {
        return ANY_FREQUENCY;
    }
    let min = ranges
        .iter()
        .map(|range| range.min)
        .fold(f64::INFINITY, f64::min);
    let max = ranges
        .iter()
        .map(|range| range.max)
        .fold(f64::NEG_INFINITY, f64::max);
    (min, max)
}

#[must_use]
pub fn in_tuning_range(hz: f64, range: (f64, f64)) -> Option<f64> {
    (hz >= range.0 && hz <= range.1).then_some(hz)
}

#[must_use]
pub fn heard_hz(frequency_hz: f64, shift_hz: Option<f64>) -> f64 {
    frequency_hz + shift_hz.unwrap_or(0.0).round()
}

#[must_use]
pub fn lane_center_hz(set: &DeviceSet, stream: u32) -> Option<f64> {
    if let Some(lane) = set.virtual_lanes.iter().find(|lane| lane.stream == stream) {
        return Some(lane.center_hz);
    }
    let own = set
        .settings
        .streams
        .iter()
        .find(|entry| entry.stream == stream)
        .and_then(|entry| entry.center_hz)
        .filter(|_| set.capabilities.per_stream.tuning);
    own.or(set.settings.center_hz)
}

#[must_use]
pub fn lane_rate_hz(set: &DeviceSet, stream: u32) -> Option<f64> {
    match set.virtual_lanes.iter().find(|lane| lane.stream == stream) {
        Some(lane) => Some(lane.sample_rate),
        None => set.settings.sample_rate,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceStatus {
    pub text: String,
    pub title: String,
    pub warn: bool,
}

impl FaceStatus {
    fn plain(text: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            title: title.into(),
            warn: false,
        }
    }
}

#[must_use]
pub fn face_status(
    live: bool,
    binding: ChannelBinding,
    miss: Option<BandMiss>,
    driver: Option<String>,
    carrier: Option<String>,
) -> Option<FaceStatus> {
    if !live {
        return (binding != ChannelBinding::Unwired)
            .then(|| FaceStatus::plain(binding.status(), binding.hint()));
    }
    if let Some(driver) = driver {
        return Some(FaceStatus::plain(
            driver,
            "Tuned by the node on its control input",
        ));
    }
    if let Some(miss) = miss {
        let said = band_miss_said(miss);
        return Some(FaceStatus {
            text: said.label,
            title: said.title,
            warn: true,
        });
    }
    carrier.map(|carrier| FaceStatus::plain(carrier, "The radio carrying this decoder now"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_reads_the_binding_before_anything_live() {
        assert_eq!(
            face_status(false, ChannelBinding::Unwired, None, None, None),
            None
        );
        assert_eq!(
            face_status(
                false,
                ChannelBinding::RadioClosed,
                Some(BandMiss::TunedAway),
                None,
                None
            ),
            Some(FaceStatus::plain("radio closed", "Its radio is not open"))
        );
        assert_eq!(
            face_status(
                true,
                ChannelBinding::NotStarted,
                Some(BandMiss::TunedAway),
                Some(String::from("scanning")),
                None
            ),
            Some(FaceStatus::plain(
                "scanning",
                "Tuned by the node on its control input"
            ))
        );
        assert_eq!(
            face_status(
                true,
                ChannelBinding::NotStarted,
                Some(BandMiss::OffTuner),
                None,
                Some(String::from("RTL"))
            ),
            Some(FaceStatus {
                text: String::from("off tuner"),
                title: String::from("Outside the radio's tuning range"),
                warn: true
            })
        );
        assert_eq!(
            face_status(
                true,
                ChannelBinding::NotStarted,
                None,
                None,
                Some(String::from("RTL"))
            ),
            Some(FaceStatus::plain(
                "RTL",
                "The radio carrying this decoder now"
            ))
        );
        assert_eq!(
            face_status(true, ChannelBinding::NotStarted, None, None, None),
            None
        );
    }

    #[test]
    fn the_dial_shows_where_the_channel_is_heard() {
        assert_eq!(heard_hz(145_500_000.0, None), 145_500_000.0);
        assert_eq!(heard_hz(145_500_000.0, Some(1_234.6)), 145_501_235.0);
        assert_eq!(heard_hz(145_500_000.0, Some(-20.2)), 145_499_980.0);
    }

    #[test]
    fn the_channel_keys_match_the_web_hotkeys() {
        assert_eq!(hotkey_of("m"), Some(Hotkey::CycleMode(1)));
        assert_eq!(hotkey_of("M"), Some(Hotkey::CycleMode(-1)));
        assert_eq!(hotkey_of("-"), Some(Hotkey::Squelch(-1)));
        assert_eq!(hotkey_of("="), Some(Hotkey::Squelch(1)));
        assert_eq!(hotkey_of("+"), Some(Hotkey::Squelch(1)));
        assert_eq!(hotkey_of("s"), Some(Hotkey::ToggleSquelch));
        assert_eq!(hotkey_of("x"), None);
    }

    #[test]
    fn the_tuning_range_spans_every_range_the_radio_reaches() {
        let capabilities: Capabilities = serde_json::from_value(serde_json::json!({
            "freq_ranges": [{ "min": 24e6, "max": 1.7e9 }, { "min": 500e3, "max": 28e6 }],
            "sample_rates": [], "gains": [], "antennas": [], "bandwidths": []
        }))
        .expect("capabilities");
        assert_eq!(tuning_range(&capabilities), (500e3, 1.7e9));
        let none: Capabilities = serde_json::from_value(serde_json::json!({
            "freq_ranges": [], "sample_rates": [], "gains": [], "antennas": [], "bandwidths": []
        }))
        .expect("capabilities");
        assert_eq!(tuning_range(&none), ANY_FREQUENCY);
        assert_eq!(in_tuning_range(100e6, (0.0, 6e9)), Some(100e6));
        assert_eq!(in_tuning_range(7e9, (0.0, 6e9)), None);
    }
}

use serde::{Deserialize, Serialize};

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::share;
use sdrmm_wire::present::text_of;
use sdrmm_wire::{
    AirtimeSpan, Capability, DecoderSettings, EventContract, EventKind, Hue, KindSpec,
};

pub const DEFAULT_WIFI_MARGIN_DB: f32 = 6.0;

fn default_wifi_margin_db() -> f32 {
    DEFAULT_WIFI_MARGIN_DB
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WifiOccupancyParams {
    #[serde(default)]
    pub span: AirtimeSpan,
    #[serde(default = "default_wifi_margin_db")]
    pub margin_db: f32,
}

impl Default for WifiOccupancyParams {
    fn default() -> Self {
        Self {
            span: AirtimeSpan::default(),
            margin_db: default_wifi_margin_db(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiBand {
    Ghz2_4,
    Ghz5,
    Ghz6,
}

impl WifiBand {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ghz2_4 => "2.4 GHz",
            Self::Ghz5 => "5 GHz",
            Self::Ghz6 => "6 GHz",
        }
    }

    /// The band in a field key; channel numbers repeat across bands.
    const fn key(self) -> &'static str {
        match self {
            Self::Ghz2_4 => "2g4",
            Self::Ghz5 => "5g",
            Self::Ghz6 => "6g",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WifiChannelLoad {
    pub band: WifiBand,
    pub number: u8,
    pub centre_hz: f64,
    pub busy: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level_dbfs: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_dbfs: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_dbfs: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WifiOccupancyReport {
    pub window_ms: u32,
    pub measured: f32,
    pub channels: Vec<WifiChannelLoad>,
}

impl WifiOccupancyReport {
    #[must_use]
    pub fn summary(&self) -> String {
        if self.channels.is_empty() {
            return "no Wi-Fi channel in view".to_owned();
        }
        self.channels
            .iter()
            .map(|channel| format!("ch {} {:.0}%", channel.number, channel.busy * 100.0))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

impl EventPayload for WifiOccupancyReport {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let mut shown: Vec<Field> = self.channels.iter().filter_map(channel_field).collect();
        shown.extend(fields(&[("measured", "Measured", share(self.measured))]));
        Presentation {
            fields: shown,
            ..Presentation::of(self.summary())
        }
    }
}

fn channel_field(channel: &WifiChannelLoad) -> Option<Field> {
    let mut parts = vec![format!("{:.0}% busy", channel.busy * 100.0)];
    parts.extend(channel.level_dbfs.map(|level| format!("{level:.0} dBFS")));
    parts.extend(
        channel
            .floor_dbfs
            .map(|floor| format!("floor {floor:.0} dBFS")),
    );
    let key = format!("ch_{}_{}", channel.band.key(), channel.number);
    let label = format!("{} ch {}", channel.band.label(), channel.number);
    text_of(parts.join(" · ")).map(|value| Field::new(key, label, value))
}

pub(crate) fn wifi_occupancy_kind() -> KindSpec {
    KindSpec::of::<WifiOccupancyReport>("Wi-Fi load", "Wi-Fi load", Hue::Muted)
        .caps(&[Capability::Log])
        .build()
}

impl EventContract for WifiOccupancyReport {
    const KIND: EventKind = EventKind::named("wifi_occupancy");
}

impl DecoderSettings for WifiOccupancyParams {
    const TYPE_ID: &'static str = "wifi_occupancy";
}

#[cfg(test)]
mod model_tests {
    use sdrmm_wire::present::{Unit, number};

    use super::*;

    #[test]
    fn empty_settings_take_the_defaults() {
        let wifi: WifiOccupancyParams = serde_json::from_str("{}").unwrap();
        assert_eq!(wifi, WifiOccupancyParams::default());
    }

    #[test]
    fn a_summary_reads_as_load() {
        let report = WifiOccupancyReport {
            window_ms: 1_000,
            measured: 1.0,
            channels: vec![WifiChannelLoad {
                band: WifiBand::Ghz2_4,
                number: 6,
                centre_hz: 2_437e6,
                busy: 0.314,
                level_dbfs: Some(-40.0),
                peak_dbfs: Some(-31.0),
                floor_dbfs: Some(-70.0),
            }],
        };
        assert_eq!(report.summary(), "ch 6 31%");
    }

    #[test]
    fn wifi_load_lists_each_channel() {
        let report = WifiOccupancyReport {
            window_ms: 1000,
            measured: 0.05,
            channels: vec![WifiChannelLoad {
                band: WifiBand::Ghz2_4,
                number: 6,
                centre_hz: 2.437e9,
                busy: 0.35,
                level_dbfs: Some(-40.2),
                peak_dbfs: None,
                floor_dbfs: Some(-90.0),
            }],
        };
        let view = report.present(&EventKind::named("wifi_occupancy"));
        assert_eq!(view.labels(), ["2.4 GHz ch 6", "Measured"]);
        assert_eq!(view.keys(), ["ch_2g4_6", "measured"]);
        assert_eq!(
            view.field("ch_2g4_6"),
            text_of("35% busy · -40 dBFS · floor -90 dBFS").as_ref()
        );
        assert_eq!(
            view.field("Measured"),
            number(Some(0.05_f32 * 100.0), Unit::Percent, 1).as_ref()
        );
    }

    #[test]
    fn every_band_keeps_its_own_key() {
        let channel = |band, number| WifiChannelLoad {
            band,
            number,
            centre_hz: 0.0,
            busy: 0.1,
            level_dbfs: None,
            peak_dbfs: None,
            floor_dbfs: None,
        };
        let wifi = WifiOccupancyReport {
            window_ms: 1000,
            measured: 1.0,
            channels: vec![
                channel(WifiBand::Ghz2_4, 1),
                channel(WifiBand::Ghz6, 1),
                channel(WifiBand::Ghz5, 36),
            ],
        }
        .present(&EventKind::named("wifi_occupancy"));
        assert_eq!(wifi.keys(), ["ch_2g4_1", "ch_6g_1", "ch_5g_36", "measured"]);
    }
}

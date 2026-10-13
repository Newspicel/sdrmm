use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    decode::EventKind,
    present::{
        EventPayload, Field, FieldValue, Presentation, Unit, count, fields, number, text_of,
    },
    units::hertz,
};

pub const AIRTIME_REPORT_MS: u32 = 1_000;
pub const MIN_AIRTIME_MARGIN_DB: f32 = 3.0;
pub const MAX_AIRTIME_MARGIN_DB: f32 = 30.0;
pub const SURVEY_CENTRES: usize = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AirtimeSpan {
    #[default]
    Mhz20,
    Mhz40,
    Mhz80,
}

impl AirtimeSpan {
    #[must_use]
    pub fn sample_rate_hz(self) -> f64 {
        match self {
            Self::Mhz20 => 20_000_000.0,
            Self::Mhz40 => 40_000_000.0,
            Self::Mhz80 => 80_000_000.0,
        }
    }
}

fn default_wifi_margin_db() -> f32 {
    6.0
}

fn default_survey_margin_db() -> f32 {
    10.0
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IsmSurveyParams {
    #[serde(default)]
    pub span: AirtimeSpan,
    #[serde(default = "default_survey_margin_db")]
    pub margin_db: f32,
}

impl Default for IsmSurveyParams {
    fn default() -> Self {
        Self {
            span: AirtimeSpan::default(),
            margin_db: default_survey_margin_db(),
        }
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum IsmKind {
    Wifi,
    Bluetooth,
    Ieee802154,
    MicrowaveOven,
    Narrowband,
    Wideband,
    Continuous,
}

impl IsmKind {
    pub const ALL: [Self; 7] = [
        Self::Wifi,
        Self::Bluetooth,
        Self::Ieee802154,
        Self::MicrowaveOven,
        Self::Narrowband,
        Self::Wideband,
        Self::Continuous,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Wifi => "Wi-Fi",
            Self::Bluetooth => "Bluetooth",
            Self::Ieee802154 => "802.15.4",
            Self::MicrowaveOven => "Microwave oven",
            Self::Narrowband => "Narrowband",
            Self::Wideband => "Wideband",
            Self::Continuous => "Continuous",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IsmKindLoad {
    pub kind: IsmKind,
    pub bursts: u32,
    pub airtime: f32,
    pub mean_us: f32,
    pub peak_dbfs: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub centres_mhz: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IsmSurveyReport {
    pub window_ms: u32,
    pub low_hz: f64,
    pub high_hz: f64,
    pub measured: f32,
    pub busy: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_dbfs: Option<f32>,
    pub kinds: Vec<IsmKindLoad>,
    #[serde(default)]
    pub dropped: u32,
}

impl IsmSurveyReport {
    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = vec![format!("{:.0}% busy", self.busy * 100.0)];
        parts.extend(self.kinds.iter().map(|load| {
            format!(
                "{} {} · {:.1}%",
                load.kind.label(),
                load.bursts,
                load.airtime * 100.0
            )
        }));
        parts.join(" · ")
    }
}

impl EventPayload for WifiOccupancyReport {
    fn present(&self, _kind: EventKind) -> Presentation {
        let mut shown: Vec<Field> = self.channels.iter().filter_map(channel_field).collect();
        shown.extend(fields(&[("Measured", share(self.measured))]));
        Presentation {
            fields: shown,
            ..Presentation::of(self.summary())
        }
    }
}

fn share(fraction: f32) -> Option<FieldValue> {
    let digits = if fraction < 0.1 { 1 } else { 0 };
    number(Some(fraction * 100.0), Unit::Percent, digits)
}

fn channel_field(channel: &WifiChannelLoad) -> Option<Field> {
    let mut parts = vec![format!("{:.0}% busy", channel.busy * 100.0)];
    parts.extend(channel.level_dbfs.map(|level| format!("{level:.0} dBFS")));
    parts.extend(
        channel
            .floor_dbfs
            .map(|floor| format!("floor {floor:.0} dBFS")),
    );
    text_of(parts.join(" · ")).map(|value| Field {
        label: format!("{} ch {}", channel.band.label(), channel.number),
        value,
    })
}

impl EventPayload for IsmSurveyReport {
    fn present(&self, _kind: EventKind) -> Presentation {
        let mut shown = fields(&[
            (
                "Window",
                text_of(format!("{} to {}", hertz(self.low_hz), hertz(self.high_hz))),
            ),
            ("Busy", share(self.busy)),
        ]);
        shown.extend(self.kinds.iter().filter_map(kind_field));
        shown.extend(fields(&[
            ("Floor", number(self.floor_dbfs, Unit::Dbfs, 0)),
            ("Measured", share(self.measured)),
            (
                "Dropped",
                count(Some(self.dropped).filter(|dropped| *dropped > 0)),
            ),
        ]));
        Presentation {
            fields: shown,
            ..Presentation::of(self.summary())
        }
    }
}

fn kind_field(load: &IsmKindLoad) -> Option<Field> {
    let centres = load
        .centres_mhz
        .iter()
        .map(f64::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let digits = if load.airtime < 0.1 { 1 } else { 0 };
    text_of(format!(
        "{} bursts · {:.*}% · {:.0} µs · {centres} MHz",
        load.bursts,
        digits,
        load.airtime * 100.0,
        load.mean_us
    ))
    .map(|value| Field {
        label: load.kind.label().to_owned(),
        value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_settings_take_the_defaults() {
        let wifi: WifiOccupancyParams = serde_json::from_str("{}").unwrap();
        assert_eq!(wifi, WifiOccupancyParams::default());
        let survey: IsmSurveyParams = serde_json::from_str("{}").unwrap();
        assert_eq!(survey.margin_db, 10.0);
        assert_eq!(survey.span.sample_rate_hz(), 20e6);
    }

    #[test]
    fn summaries_read_as_load() {
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
        let survey = IsmSurveyReport {
            window_ms: 1_000,
            low_hz: 2_427e6,
            high_hz: 2_447e6,
            measured: 1.0,
            busy: 0.42,
            floor_dbfs: None,
            kinds: vec![IsmKindLoad {
                kind: IsmKind::Bluetooth,
                bursts: 12,
                airtime: 0.004,
                mean_us: 376.0,
                peak_dbfs: -30.0,
                centres_mhz: vec![2_426.0],
            }],
            dropped: 0,
        };
        assert_eq!(survey.summary(), "42% busy · Bluetooth 12 · 0.4%");
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
        let view = report.present(EventKind::WifiOccupancy);
        assert_eq!(view.labels(), ["2.4 GHz ch 6", "Measured"]);
        assert_eq!(
            view.field("2.4 GHz ch 6"),
            text_of("35% busy · -40 dBFS · floor -90 dBFS").as_ref()
        );
        assert_eq!(
            view.field("Measured"),
            number(Some(0.05_f32 * 100.0), Unit::Percent, 1).as_ref()
        );
    }

    #[test]
    fn an_ism_survey_names_its_window_and_kinds() {
        let report = IsmSurveyReport {
            window_ms: 1000,
            low_hz: 2.4e9,
            high_hz: 2.48e9,
            measured: 0.5,
            busy: 0.2,
            floor_dbfs: None,
            kinds: vec![IsmKindLoad {
                kind: IsmKind::Bluetooth,
                bursts: 12,
                airtime: 0.02,
                mean_us: 380.0,
                peak_dbfs: -30.0,
                centres_mhz: vec![2402.0, 2480.0],
            }],
            dropped: 0,
        };
        let view = report.present(EventKind::IsmSurvey);
        assert_eq!(
            view.field("Window"),
            text_of("2.4 GHz to 2.48 GHz").as_ref()
        );
        assert_eq!(
            view.field("Bluetooth"),
            text_of("12 bursts · 2.0% · 380 µs · 2402, 2480 MHz").as_ref()
        );
        assert_eq!(view.field("Dropped"), None);
    }
}

use serde::{Deserialize, Serialize};

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::share;
use sdrmm_wire::present::text_of;
use sdrmm_wire::present::{Unit, count, number};
use sdrmm_wire::units::hertz;
use sdrmm_wire::{
    AirtimeSpan, Capability, DecoderSettings, EventContract, EventKind, Hue, KindSpec, KindView,
};

pub const SURVEY_CENTRES: usize = 6;

/// Long enough to catch a microwave oven's cycle or a burst of traffic.
const BUSY_TREND_S: u32 = 600;

pub const DEFAULT_SURVEY_MARGIN_DB: f32 = 10.0;

fn default_survey_margin_db() -> f32 {
    DEFAULT_SURVEY_MARGIN_DB
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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

    const fn key(self) -> &'static str {
        match self {
            Self::Wifi => "wifi",
            Self::Bluetooth => "bluetooth",
            Self::Ieee802154 => "ieee802154",
            Self::MicrowaveOven => "microwave_oven",
            Self::Narrowband => "narrowband",
            Self::Wideband => "wideband",
            Self::Continuous => "continuous",
        }
    }

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IsmKindLoad {
    pub kind: IsmKind,
    pub bursts: u32,
    pub airtime: f32,
    pub mean_us: f32,
    pub peak_dbfs: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub centres_mhz: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

impl EventPayload for IsmSurveyReport {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let mut shown = fields(&[
            (
                "window",
                "Window",
                text_of(format!("{} to {}", hertz(self.low_hz), hertz(self.high_hz))),
            ),
            ("busy", "Busy", share(self.busy)),
        ]);
        shown.extend(self.kinds.iter().filter_map(kind_field));
        shown.extend(fields(&[
            ("floor", "Floor", number(self.floor_dbfs, Unit::Dbfs, 0)),
            ("measured", "Measured", share(self.measured)),
            (
                "dropped",
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
    let digits = usize::from(load.airtime < 0.1);
    text_of(format!(
        "{} bursts · {:.*}% · {:.0} µs · {centres} MHz",
        load.bursts,
        digits,
        load.airtime * 100.0,
        load.mean_us
    ))
    .map(|value| Field::new(load.kind.key(), load.kind.label(), value))
}

pub(crate) fn ism_survey_kind() -> KindSpec {
    KindSpec::of::<IsmSurveyReport>("2.4 GHz", "2.4 GHz", Hue::Muted)
        .caps(&[Capability::Log])
        .view(KindView::meter("busy", 0.0, 100.0))
        .view(KindView::trend(&["busy"], BUSY_TREND_S))
        .build()
}

impl EventContract for IsmSurveyReport {
    const KIND: EventKind = EventKind::named("ism_survey");
}

impl DecoderSettings for IsmSurveyParams {
    const TYPE_ID: &'static str = "ism_survey";
}

#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn empty_settings_take_the_defaults() {
        let survey: IsmSurveyParams = serde_json::from_str("{}").unwrap();
        assert_eq!(survey.margin_db, 10.0);
        assert_eq!(survey.span.sample_rate_hz(), 20e6);
    }

    #[test]
    fn a_survey_summary_reads_as_load() {
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
        let view = report.present(&EventKind::named("ism_survey"));
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

    #[test]
    fn every_kind_keeps_its_own_key() {
        let survey = IsmSurveyReport {
            window_ms: 1000,
            low_hz: 2.4e9,
            high_hz: 2.48e9,
            measured: 1.0,
            busy: 0.2,
            floor_dbfs: Some(-90.0),
            kinds: IsmKind::ALL
                .iter()
                .map(|&kind| IsmKindLoad {
                    kind,
                    bursts: 1,
                    airtime: 0.01,
                    mean_us: 100.0,
                    peak_dbfs: -30.0,
                    centres_mhz: Vec::new(),
                })
                .collect(),
            dropped: 0,
        };
        sdrmm_decoder_test_support::assert_views_hold(
            &ism_survey_kind(),
            &[sdrmm_wire::DecoderEvent::of(&survey)],
        );
    }
}

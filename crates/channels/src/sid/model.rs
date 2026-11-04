use sdrmm_wire::present::key_of;
use sdrmm_wire::{Capability, DecoderSettings, EventContract, Hue, KindSpec, KindView};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::number;
use sdrmm_wire::present::only_if;
use sdrmm_wire::present::text_of;
use sdrmm_wire::units::hertz;

use super::stations::STATIONS;

/// An ionospheric disturbance builds and fades over tens of minutes.
const SID_TREND_S: u32 = 3_600;

pub const MIN_SID_REPORT_S: u16 = 1;

pub const MAX_SID_REPORT_S: u16 = 60;

pub const MIN_SID_THRESHOLD_DB: f32 = 1.0;

pub const MAX_SID_THRESHOLD_DB: f32 = 20.0;

fn default_sid_report_s() -> u16 {
    5
}

fn default_sid_threshold_db() -> f32 {
    3.0
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SidParams {
    #[serde(default = "default_sid_report_s")]
    pub report_s: u16,
    #[serde(default = "default_sid_threshold_db")]
    pub threshold_db: f32,
}

impl Default for SidParams {
    fn default() -> Self {
        Self {
            report_s: default_sid_report_s(),
            threshold_db: default_sid_threshold_db(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SidStationLevel {
    pub call: String,
    pub frequency_hz: f64,
    pub level_dbfs: f32,
    pub snr_db: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_dbfs: Option<f32>,
    #[serde(default)]
    pub disturbed: bool,
}

impl SidStationLevel {
    #[must_use]
    pub fn deviation_db(&self) -> Option<f32> {
        self.baseline_dbfs
            .map(|baseline| self.level_dbfs - baseline)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SidReport {
    pub window_ms: u32,
    pub floor_dbfs_hz: f32,
    pub stations: Vec<SidStationLevel>,
}

impl SidReport {
    #[must_use]
    pub fn summary(&self) -> String {
        if self.stations.is_empty() {
            return "no VLF station in view".to_owned();
        }
        let disturbed: Vec<String> = self
            .stations
            .iter()
            .filter(|station| station.disturbed)
            .map(|station| {
                format!(
                    "{} {:+.1} dB",
                    station.call,
                    station.deviation_db().unwrap_or_default()
                )
            })
            .collect();
        if !disturbed.is_empty() {
            return format!("SID · {}", disturbed.join(" · "));
        }
        self.stations
            .iter()
            .map(|station| format!("{} {:.0} dB", station.call, station.snr_db.round() + 0.0))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

impl EventPayload for SidReport {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let mut shown: Vec<Field> = self.stations.iter().flat_map(station_fields).collect();
        shown.extend(fields(&[(
            "floor",
            "Floor",
            text_of(format!("{:.0} dBFS/Hz", self.floor_dbfs_hz)),
        )]));
        Presentation {
            fields: shown,
            ..Presentation::of(self.summary())
        }
    }
}

/// A station's key prefix: its call and frequency in whole hertz, as one call can send on
/// two frequencies. It stays fixed however frequencies are written.
fn station_key(call: &str, frequency_hz: f64) -> String {
    format!("{}_{frequency_hz:.0}", key_of(call))
}

fn station_fields(station: &SidStationLevel) -> Vec<Field> {
    let key = station_key(&station.call, station.frequency_hz);
    let name = format!("{} {}", station.call, hertz(station.frequency_hz));
    fields(&[
        (
            &format!("{key}_level"),
            &name,
            number(Some(station.level_dbfs), Unit::Dbfs, 1),
        ),
        (
            &format!("{key}_snr"),
            &format!("{name} SNR"),
            number(Some(station.snr_db.round() + 0.0), Unit::Db, 0),
        ),
        (
            &format!("{key}_deviation"),
            &format!("{name} Δ"),
            number(station.deviation_db(), Unit::Db, 1),
        ),
        (
            &format!("{key}_sid"),
            &format!("{name} SID"),
            only_if(station.disturbed),
        ),
    ])
}

/// `measure` of every station the decoder knows, plotted over time.
fn station_trend(measure: &str) -> KindView {
    let keys: Vec<String> = STATIONS
        .iter()
        .map(|station| {
            format!(
                "{}_{measure}",
                station_key(station.call, station.frequency_hz)
            )
        })
        .collect();
    let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
    KindView::trend(&keys, SID_TREND_S)
}

pub(crate) fn sid_kind() -> KindSpec {
    KindSpec::of::<SidReport>("SID", "SID", Hue::Muted)
        .caps(&[Capability::Log])
        .view(station_trend("level"))
        .view(station_trend("deviation"))
        .build()
}

impl EventContract for SidReport {
    const KIND: EventKind = EventKind::named("sid");
}

impl DecoderSettings for SidParams {
    const TYPE_ID: &'static str = "sid";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use crate::noise_figure::NoiseFigureReport;
    use crate::per::PerReport;

    #[test]
    fn summaries_lead_with_the_measurement() {
        let mut nf = NoiseFigureReport {
            window_ms: 1_000,
            level_dbfs: -60.04,
            settled_s: 3.0,
            source_on: None,
            hot_dbfs: None,
            cold_dbfs: None,
            y_db: None,
            nf_db: None,
            temperature_k: None,
        };
        assert_eq!(nf.summary(), "-60.0 dBFS · toggle the noise source");
        nf.y_db = Some(10.5);
        nf.nf_db = Some(4.75);
        assert_eq!(nf.summary(), "NF 4.75 dB · Y 10.50 dB");
        let per = PerReport {
            received: 990,
            expected: 1_000,
            missed: 10,
            per: 0.01,
            bits: 256_000,
            bit_errors: 80,
            ber: Some(3.125e-4),
            ..PerReport::default()
        };
        assert_eq!(per.summary(), "PER 1.00% · 990/1000 · BER 3.1e-4");
        let clean = PerReport {
            ber: Some(0.0),
            ..per.clone()
        };
        assert!(clean.summary().ends_with("BER 0"), "{}", clean.summary());
        let mut sid = SidReport {
            window_ms: 5_000,
            floor_dbfs_hz: -110.0,
            stations: vec![SidStationLevel {
                call: "DHO38".to_owned(),
                frequency_hz: 23_400.0,
                level_dbfs: -50.0,
                snr_db: 40.2,
                baseline_dbfs: Some(-56.0),
                disturbed: false,
            }],
        };
        assert_eq!(sid.summary(), "DHO38 40 dB");
        sid.stations[0].snr_db = -0.3;
        assert_eq!(sid.summary(), "DHO38 0 dB");
        sid.stations[0].snr_db = 40.2;
        sid.stations[0].disturbed = true;
        assert_eq!(sid.summary(), "SID · DHO38 +6.0 dB");
    }

    #[test]
    fn sid_lists_each_station_and_the_floor() {
        let view = SidReport {
            window_ms: 5000,
            floor_dbfs_hz: -150.0,
            stations: vec![SidStationLevel {
                call: "DHO38".to_owned(),
                frequency_hz: 23_400.0,
                level_dbfs: -40.0,
                snr_db: 30.4,
                baseline_dbfs: Some(-46.0),
                disturbed: true,
            }],
        }
        .present(&EventKind::named("sid"));
        assert_eq!(
            view.labels(),
            [
                "DHO38 23.4 kHz",
                "DHO38 23.4 kHz SNR",
                "DHO38 23.4 kHz Δ",
                "DHO38 23.4 kHz SID",
                "Floor"
            ]
        );
        assert_eq!(
            view.keys(),
            [
                "dho38_23400_level",
                "dho38_23400_snr",
                "dho38_23400_deviation",
                "dho38_23400_sid",
                "floor"
            ]
        );
        assert_eq!(
            view.field("dho38_23400_level"),
            number(Some(-40.0_f32), Unit::Dbfs, 1).as_ref()
        );
        assert_eq!(
            view.field("dho38_23400_deviation"),
            number(Some(6.0_f32), Unit::Db, 1).as_ref()
        );
    }

    #[test]
    fn the_trends_name_a_level_and_deviation_for_every_station() {
        let stations = STATIONS
            .iter()
            .map(|station| SidStationLevel {
                call: station.call.to_owned(),
                frequency_hz: station.frequency_hz,
                level_dbfs: -50.0,
                snr_db: 20.0,
                baseline_dbfs: Some(-52.0),
                disturbed: false,
            })
            .collect();
        let report = SidReport {
            window_ms: 5_000,
            floor_dbfs_hz: -120.0,
            stations,
        };
        crate::testutil::assert_views_hold(&sid_kind(), &[sdrmm_wire::DecoderEvent::of(&report)]);
    }

    #[test]
    fn a_call_on_two_frequencies_keeps_two_keys() {
        let keys: Vec<String> = STATIONS
            .iter()
            .map(|station| station_key(station.call, station.frequency_hz))
            .collect();
        assert!(keys.contains(&"hwu_18300".to_owned()), "{keys:?}");
        assert!(keys.contains(&"hwu_21750".to_owned()), "{keys:?}");
        let unique: std::collections::BTreeSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len());
    }
}

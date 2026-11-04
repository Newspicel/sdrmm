use sdrmm_wire::{Capability, DecoderSettings, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::text_of;
use sdrmm_wire::units::hertz;

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
        let mut shown: Vec<Field> = self.stations.iter().filter_map(station_field).collect();
        shown.extend(fields(&[(
            "Floor",
            text_of(format!("{:.0} dBFS/Hz", self.floor_dbfs_hz)),
        )]));
        Presentation {
            fields: shown,
            ..Presentation::of(self.summary())
        }
    }
}

fn station_field(station: &SidStationLevel) -> Option<Field> {
    let mut parts = vec![
        format!("{:.1} dBFS", station.level_dbfs),
        format!("SNR {:.0} dB", station.snr_db.round() + 0.0),
    ];
    parts.extend(
        station
            .deviation_db()
            .map(|deviation| format!("{deviation:+.1} dB")),
    );
    if station.disturbed {
        parts.push("SID".to_owned());
    }
    text_of(parts.join(" · ")).map(|value| Field {
        label: format!("{} {}", station.call, hertz(station.frequency_hz)),
        value,
    })
}

pub(crate) fn sid_kind() -> KindSpec {
    KindSpec::of::<SidReport>("SID", "SID", Hue::Muted)
        .caps(&[Capability::Log])
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
        assert_eq!(view.labels(), ["DHO38 23.4 kHz", "Floor"]);
        assert_eq!(
            view.field("DHO38 23.4 kHz"),
            text_of("-40.0 dBFS · SNR 30 dB · +6.0 dB · SID").as_ref()
        );
    }
}

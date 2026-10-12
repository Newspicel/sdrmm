use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    decode::EventKind,
    present::{EventPayload, Field, Presentation, Unit, count, fields, number, text_of},
    units::hertz,
};

pub const MEASUREMENT_REPORT_MS: u32 = 1_000;
pub const MIN_ENR_DB: f32 = 1.0;
pub const MAX_ENR_DB: f32 = 40.0;
pub const MIN_PER_PAYLOAD_BYTES: u16 = 1;
pub const MAX_PER_PAYLOAD_BYTES: u16 = 255;
pub const MIN_SID_REPORT_S: u16 = 1;
pub const MAX_SID_REPORT_S: u16 = 60;
pub const MIN_SID_THRESHOLD_DB: f32 = 1.0;
pub const MAX_SID_THRESHOLD_DB: f32 = 20.0;

fn default_enr_db() -> f32 {
    15.0
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NoiseFigureParams {
    #[serde(default = "default_enr_db")]
    pub enr_db: f32,
}

impl Default for NoiseFigureParams {
    fn default() -> Self {
        Self {
            enr_db: default_enr_db(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NoiseFigureReport {
    pub window_ms: u32,
    pub level_dbfs: f32,
    pub settled_s: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_on: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hot_dbfs: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cold_dbfs: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y_db: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nf_db: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature_k: Option<f32>,
}

impl NoiseFigureReport {
    #[must_use]
    pub fn summary(&self) -> String {
        match (self.nf_db, self.y_db) {
            (Some(nf), Some(y)) => format!("NF {nf:.2} dB · Y {y:.2} dB"),
            (None, Some(y)) => format!("Y {y:.2} dB, above the ENR"),
            _ => format!("{:.1} dBFS · toggle the noise source", self.level_dbfs),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PerBaud {
    Baud1200,
    Baud2400,
    Baud4800,
    #[default]
    Baud9600,
}

impl PerBaud {
    pub const ALL: [Self; 4] = [
        Self::Baud1200,
        Self::Baud2400,
        Self::Baud4800,
        Self::Baud9600,
    ];

    #[must_use]
    pub fn rate(self) -> f64 {
        match self {
            Self::Baud1200 => 1_200.0,
            Self::Baud2400 => 2_400.0,
            Self::Baud4800 => 4_800.0,
            Self::Baud9600 => 9_600.0,
        }
    }
}

fn default_per_payload_bytes() -> u16 {
    32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PerParams {
    #[serde(default)]
    pub baud: PerBaud,
    #[serde(default = "default_per_payload_bytes")]
    pub payload_bytes: u16,
}

impl Default for PerParams {
    fn default() -> Self {
        Self {
            baud: PerBaud::default(),
            payload_bytes: default_per_payload_bytes(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PerReport {
    pub received: u64,
    pub errored: u64,
    pub missed: u64,
    pub expected: u64,
    pub per: f32,
    pub bits: u64,
    pub bit_errors: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ber: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sequence: Option<u32>,
    #[serde(default)]
    pub restarts: u32,
}

impl PerReport {
    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = vec![
            format!("PER {:.2}%", self.per * 100.0),
            format!("{}/{}", self.received, self.expected),
        ];
        match self.ber {
            Some(0.0) => parts.push("BER 0".to_owned()),
            Some(ber) => parts.push(format!("BER {ber:.1e}")),
            None => {}
        }
        parts.join(" · ")
    }
}

fn default_sid_report_s() -> u16 {
    5
}

fn default_sid_threshold_db() -> f32 {
    3.0
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
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

impl EventPayload for NoiseFigureReport {
    fn present(&self, _kind: EventKind) -> Presentation {
        let source = self.source_on.map(|on| if on { "on" } else { "off" });
        Presentation {
            fields: fields(&[
                ("Noise figure", number(self.nf_db, Unit::Db, 2)),
                (
                    "Noise temperature",
                    number(self.temperature_k, Unit::Kelvin, 0),
                ),
                ("Y-factor", number(self.y_db, Unit::Db, 2)),
                ("Source on", number(self.hot_dbfs, Unit::Dbfs, 2)),
                ("Source off", number(self.cold_dbfs, Unit::Dbfs, 2)),
                ("Level", number(Some(self.level_dbfs), Unit::Dbfs, 2)),
                ("Source", source.and_then(text_of)),
                ("Settled", number(Some(self.settled_s), Unit::Seconds, 1)),
            ]),
            ..Presentation::of(self.summary())
        }
    }
}

impl EventPayload for PerReport {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            fields: fields(&[
                ("PER", number(Some(self.per * 100.0), Unit::Percent, 2)),
                (
                    "Received",
                    text_of(format!("{} of {}", self.received, self.expected)),
                ),
                ("Missed", count(Some(self.missed as f64))),
                ("CRC failed", count(Some(self.errored as f64))),
                (
                    "BER",
                    self.ber.and_then(|ber| text_of(format!("{ber:.2e}"))),
                ),
                (
                    "Bit errors",
                    text_of(format!("{} of {}", self.bit_errors, self.bits)),
                ),
                ("Last frame", count(self.last_sequence)),
                (
                    "Restarts",
                    count(Some(self.restarts).filter(|restarts| *restarts > 0)),
                ),
            ]),
            ..Presentation::of(self.summary())
        }
    }
}

impl EventPayload for SidReport {
    fn present(&self, _kind: EventKind) -> Presentation {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_settings_take_the_defaults() {
        let nf: NoiseFigureParams = serde_json::from_str("{}").unwrap();
        assert_eq!(nf.enr_db, 15.0);
        let per: PerParams = serde_json::from_str("{}").unwrap();
        assert_eq!(per, PerParams::default());
        assert_eq!(per.baud.rate(), 9_600.0);
        let sid: SidParams = serde_json::from_str("{}").unwrap();
        assert_eq!(sid, SidParams::default());
        assert_eq!(serde_json::to_value(PerBaud::Baud1200).unwrap(), "baud1200");
    }

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
    fn a_noise_figure_report_shows_the_measured_figures() {
        let view = NoiseFigureReport {
            window_ms: 1000,
            level_dbfs: -60.0,
            settled_s: 2.5,
            source_on: Some(true),
            hot_dbfs: Some(-50.0),
            cold_dbfs: Some(-60.0),
            y_db: Some(10.0),
            nf_db: Some(5.12),
            temperature_k: Some(650.0),
        }
        .present(EventKind::NoiseFigure);
        assert_eq!(
            view.field("Noise figure"),
            number(Some(5.12_f32), Unit::Db, 2).as_ref()
        );
        assert_eq!(
            view.field("Noise temperature"),
            number(Some(650.0_f32), Unit::Kelvin, 0).as_ref()
        );
        assert_eq!(view.field("Source"), text_of("on").as_ref());
    }

    #[test]
    fn a_per_report_counts_frames_and_bits() {
        let view = PerReport {
            received: 98,
            errored: 1,
            missed: 1,
            expected: 100,
            per: 0.02,
            bits: 25_088,
            bit_errors: 3,
            ber: Some(0.000_12),
            last_sequence: Some(99),
            restarts: 0,
        }
        .present(EventKind::Per);
        assert_eq!(view.field("Received"), text_of("98 of 100").as_ref());
        assert_eq!(view.field("BER"), text_of("1.20e-4").as_ref());
        assert_eq!(view.field("Restarts"), None);
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
        .present(EventKind::Sid);
        assert_eq!(view.labels(), ["DHO38 23.4 kHz", "Floor"]);
        assert_eq!(
            view.field("DHO38 23.4 kHz"),
            text_of("-40.0 dBFS · SNR 30 dB · +6.0 dB · SID").as_ref()
        );
    }
}

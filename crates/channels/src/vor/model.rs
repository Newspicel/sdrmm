use sdrmm_wire::{Capability, DecoderSettings, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::number;
use sdrmm_wire::present::percent;
use sdrmm_wire::present::position;
use sdrmm_wire::present::text;
use sdrmm_wire::present::trimmed;

pub const MIN_NAVAID_REPORT_MS: u32 = 250;

pub const MAX_NAVAID_REPORT_MS: u32 = 5_000;

pub(crate) fn default_navaid_report_ms() -> u32 {
    500
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VorParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station_lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station_lon: Option<f64>,
    #[serde(default)]
    pub magnetic_declination_deg: f64,
    #[serde(default = "default_navaid_report_ms")]
    pub report_ms: u32,
}

impl Default for VorParams {
    fn default() -> Self {
        Self {
            station: None,
            station_lat: None,
            station_lon: None,
            magnetic_declination_deg: 0.0,
            report_ms: default_navaid_report_ms(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VorReading {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station_lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station_lon: Option<f64>,
    pub magnetic_declination_deg: f64,
    pub radial_deg: f64,
    pub variable_phase_deg: f64,
    pub reference_phase_deg: f64,
    pub signal_db: f32,
    pub confidence: f32,
}

impl EventPayload for VorReading {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let station = self.station.as_deref().unwrap_or("VOR");
        Presentation {
            entity: trimmed(self.station.as_deref()),
            fields: vor_fields(self),
            ..Presentation::of(format!(
                "{station} · {:03.1}° radial · {:.0}%",
                self.radial_deg,
                self.confidence * 100.0
            ))
        }
    }
}

fn vor_fields(v: &VorReading) -> Vec<Field> {
    fields(&[
        ("Station", text(v.station.as_deref())),
        ("Radial", number(Some(v.radial_deg), Unit::Degrees, 2)),
        (
            "Variable phase",
            number(Some(v.variable_phase_deg), Unit::Degrees, 2),
        ),
        (
            "Reference phase",
            number(Some(v.reference_phase_deg), Unit::Degrees, 2),
        ),
        (
            "Declination",
            number(Some(v.magnetic_declination_deg), Unit::Degrees, 1),
        ),
        ("Station position", position(v.station_lat, v.station_lon)),
        ("Signal", number(Some(v.signal_db), Unit::Db, 1)),
        ("Confidence", percent(v.confidence)),
    ])
}

pub(crate) fn vor_kind() -> KindSpec {
    KindSpec::of::<VorReading>("VOR", "Beacons", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station])
        .sighting()
        .build()
}

impl DecoderSettings for VorParams {
    const TYPE_ID: &'static str = "vor";
}

impl EventContract for VorReading {
    const KIND: EventKind = EventKind::named("vor");
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::LatLon;
    use sdrmm_wire::present::FieldValue;

    #[test]
    fn vor_shows_its_phases_and_station() {
        let reading = VorReading {
            station: Some("BOT".to_owned()),
            station_lat: Some(51.5),
            station_lon: Some(7.1),
            magnetic_declination_deg: -2.5,
            radial_deg: 123.456,
            variable_phase_deg: 10.0,
            reference_phase_deg: 20.0,
            signal_db: 30.25,
            confidence: 0.874,
        };
        let view = reading.present(&EventKind::named("vor"));
        assert_eq!(
            view.labels(),
            [
                "Station",
                "Radial",
                "Variable phase",
                "Reference phase",
                "Declination",
                "Station position",
                "Signal",
                "Confidence"
            ]
        );
        assert_eq!(
            view.field("Station position"),
            Some(&FieldValue::Position(LatLon {
                lat: 51.5,
                lon: 7.1
            }))
        );
        assert_eq!(view.field("Confidence"), percent(0.874).as_ref());
    }
}

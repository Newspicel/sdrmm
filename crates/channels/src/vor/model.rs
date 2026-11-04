use sdrmm_wire::{Capability, DecoderSettings, EventContract, Hue, KindSpec, Symbol};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::present::BearingRay;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::Place;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Shape;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::bearing;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::lat_lon;
use sdrmm_wire::present::number;
use sdrmm_wire::present::percent;
use sdrmm_wire::present::position;
use sdrmm_wire::present::text;
use sdrmm_wire::present::trimmed;

pub const MIN_NAVAID_REPORT_MS: u32 = 250;

pub const MAX_NAVAID_REPORT_MS: u32 = 5_000;

/// Annex 10 holds a VOR's radial to about 2 degrees; a weaker reading widens it.
const RADIAL_SIGMA_DEG: f32 = 2.0;
const MIN_SIGMA_CONFIDENCE: f32 = 0.1;
/// Far enough to cross a radial from a VOR heard at the edge of its service volume.
const RADIAL_RAY_M: f32 = 200_000.0;

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
        let ray = self.ray();
        Presentation {
            entity: trimmed(self.station.as_deref()),
            place: ray.map(|ray| Place::at(ray.from)),
            bearing: ray,
            shapes: ray.map(radial_shape).into_iter().collect(),
            fields: vor_fields(self),
            ..Presentation::of(format!(
                "{station} · {:03.1}° radial · {:.0}%",
                self.radial_deg,
                self.confidence * 100.0
            ))
        }
    }
}

impl VorReading {
    /// The radial as a true bearing from the station toward the receiver, so radials from two
    /// stations cross where the receiver is.
    #[must_use]
    pub fn ray(&self) -> Option<BearingRay> {
        Some(BearingRay {
            from: lat_lon(self.station_lat, self.station_lon)?,
            true_deg: bearing(Some(self.radial_deg + self.magnetic_declination_deg))?,
            sigma_deg: Some(RADIAL_SIGMA_DEG / self.confidence.max(MIN_SIGMA_CONFIDENCE)),
            confidence: self.confidence,
        })
    }
}

fn radial_shape(ray: BearingRay) -> Shape {
    Shape::Ray {
        from: ray.from,
        bearing_deg: ray.true_deg,
        sigma_deg: ray.sigma_deg,
        length_m: RADIAL_RAY_M,
        faint: false,
    }
}

fn vor_fields(v: &VorReading) -> Vec<Field> {
    fields(&[
        ("station", "Station", text(v.station.as_deref())),
        (
            "radial",
            "Radial",
            number(Some(v.radial_deg), Unit::Degrees, 2),
        ),
        (
            "variable_phase",
            "Variable phase",
            number(Some(v.variable_phase_deg), Unit::Degrees, 2),
        ),
        (
            "reference_phase",
            "Reference phase",
            number(Some(v.reference_phase_deg), Unit::Degrees, 2),
        ),
        (
            "declination",
            "Declination",
            number(Some(v.magnetic_declination_deg), Unit::Degrees, 1),
        ),
        (
            "station_position",
            "Station position",
            position(v.station_lat, v.station_lon),
        ),
        ("signal", "Signal", number(Some(v.signal_db), Unit::Db, 1)),
        ("confidence", "Confidence", percent(v.confidence)),
    ])
}

pub(crate) fn vor_kind() -> KindSpec {
    KindSpec::of::<VorReading>("VOR", "Beacons", Hue::Muted)
        .caps(&[
            Capability::Log,
            Capability::Station,
            Capability::Geo,
            Capability::Bearing,
        ])
        .symbol(Symbol::BEACON)
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

    fn reading() -> VorReading {
        VorReading {
            station: Some("BOT".to_owned()),
            station_lat: Some(51.5),
            station_lon: Some(7.1),
            magnetic_declination_deg: -2.5,
            radial_deg: 123.456,
            variable_phase_deg: 10.0,
            reference_phase_deg: 20.0,
            signal_db: 30.25,
            confidence: 0.874,
        }
    }

    #[test]
    fn vor_shows_its_phases_and_station() {
        let view = reading().present(&EventKind::named("vor"));
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

    #[test]
    fn a_located_radial_is_a_true_bearing_from_the_station() {
        let view = reading().present(&EventKind::named("vor"));
        let ray = view.bearing.expect("a located VOR shows its radial");
        assert_eq!(
            ray.from,
            LatLon {
                lat: 51.5,
                lon: 7.1
            }
        );
        assert!((f64::from(ray.true_deg) - 120.956).abs() < 1e-3, "{ray:?}");
        let sigma = ray.sigma_deg.expect("sigma");
        assert!(sigma > RADIAL_SIGMA_DEG && sigma < 2.5, "{sigma}");
        assert!(matches!(
            view.shapes.as_slice(),
            [Shape::Ray { length_m, .. }] if *length_m == RADIAL_RAY_M
        ));
        assert_eq!(view.place.map(|place| place.at), Some(ray.from));
        assert!(vor_kind().entry.has(Capability::Bearing));
        assert!(vor_kind().entry.has(Capability::Geo));
    }

    #[test]
    fn a_radial_without_a_station_position_has_no_ray() {
        let unplaced = VorReading {
            station_lat: None,
            station_lon: None,
            ..reading()
        };
        let view = unplaced.present(&EventKind::named("vor"));
        assert_eq!(view.bearing, None);
        assert_eq!(view.place, None);
        assert!(view.shapes.is_empty());
    }

    #[test]
    fn a_weak_reading_widens_its_radial() {
        let weak = VorReading {
            confidence: 0.2,
            ..reading()
        };
        let sigma = weak.ray().and_then(|ray| ray.sigma_deg).expect("sigma");
        assert!((sigma - 10.0).abs() < 1e-4, "{sigma}");
    }
}

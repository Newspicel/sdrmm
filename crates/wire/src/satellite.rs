use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    LatLon,
    decode::EventKind,
    present::{
        EventPayload, FieldValue, LineStyle, Place, Presentation, Shape, Unit, fields, flag,
        number, text, trimmed,
    },
};

pub const MAX_TLE_LEN: usize = 256;
pub const MAX_TRANSMITTER_ID_LEN: usize = 64;
pub const MAX_SATELLITE_HZ: f64 = 300e9;
pub const MAX_CATALOG_RESULTS: usize = 64;
pub const MAX_SATELLITE_QUERY_LEN: usize = 64;
pub const SATELLITE_CATALOG_SOURCE: &str = "CelesTrak";
pub const SATELLITE_CATALOG_URL: &str = "https://celestrak.org/NORAD/elements/gp.php";
pub const TRANSMITTER_SOURCE: &str = "SatNOGS DB";
pub const TRANSMITTER_URL: &str = "https://db.satnogs.org/api/transmitters/";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SatelliteNode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downlink_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uplink_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transmitter: Option<String>,
    #[serde(default)]
    pub tuning_locked: bool,
}

impl SatelliteNode {
    #[must_use]
    pub fn valid(&self) -> bool {
        let frequency = |hz: Option<f64>| {
            hz.is_none_or(|hz| hz.is_finite() && hz > 0.0 && hz <= MAX_SATELLITE_HZ)
        };
        self.tle
            .as_ref()
            .is_none_or(|tle| !tle.trim().is_empty() && tle.len() <= MAX_TLE_LEN)
            && frequency(self.downlink_hz)
            && frequency(self.uplink_hz)
            && self
                .transmitter
                .as_ref()
                .is_none_or(|id| !id.is_empty() && id.len() <= MAX_TRANSMITTER_ID_LEN)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SatelliteLook {
    pub azimuth_deg: f64,
    pub elevation_deg: f64,
    pub range_km: f64,
    pub range_rate_km_s: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SatellitePass {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aos: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub los: Option<i64>,
    pub max_elevation_deg: f64,
    pub max_at: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SkyPoint {
    pub azimuth_deg: f64,
    pub elevation_deg: f64,
}

impl SkyPoint {
    #[must_use]
    pub fn off_by(self, other: Self) -> f64 {
        (self.azimuth_deg - other.azimuth_deg)
            .abs()
            .max((self.elevation_deg - other.elevation_deg).abs())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PassPoint {
    pub at: i64,
    pub azimuth_deg: f64,
    pub elevation_deg: f64,
}

impl PassPoint {
    #[must_use]
    pub const fn sky(self) -> SkyPoint {
        SkyPoint {
            azimuth_deg: self.azimuth_deg,
            elevation_deg: self.elevation_deg,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SatelliteGround {
    pub at: LatLon,
    pub altitude_km: f64,
    pub footprint_km: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SatelliteStatus {
    pub node: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tle_age_days: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground: Option<SatelliteGround>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ground_track: Vec<LatLon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look: Option<SatelliteLook>,
    #[serde(default)]
    pub visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doppler_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doppler_rate_hz_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uplink_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_pass: Option<SatellitePass>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pass_track: Vec<PassPoint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub driving: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(
    Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema, utoipa::IntoParams,
)]
#[into_params(parameter_in = Query)]
pub struct SatelliteCatalogQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CatalogSatellite {
    pub name: String,
    pub catalog: String,
    pub tle: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SatelliteCatalogResponse {
    pub satellites: Vec<CatalogSatellite>,
    pub source: String,
    #[serde(default)]
    pub fetched: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Transmitter {
    pub id: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downlink_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uplink_hz: Option<f64>,
    pub alive: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TransmittersResponse {
    pub transmitters: Vec<Transmitter>,
    pub source: String,
    #[serde(default)]
    pub fetched: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_miss_is_the_larger_axis_in_mount_degrees() {
        let point = |azimuth_deg, elevation_deg| SkyPoint {
            azimuth_deg,
            elevation_deg,
        };
        assert!((point(359.0, 10.0).off_by(point(361.0, 10.0)) - 2.0).abs() < 1e-9);
        assert!((point(90.0, 10.0).off_by(point(91.0, 40.0)) - 30.0).abs() < 1e-9);
    }

    #[test]
    fn a_blank_node_is_valid_and_waits_for_elements() {
        assert!(SatelliteNode::default().valid());
    }

    #[test]
    fn nonsense_settings_are_refused() {
        let bad = [
            SatelliteNode {
                tle: Some(" ".to_owned()),
                ..SatelliteNode::default()
            },
            SatelliteNode {
                tle: Some("x".repeat(MAX_TLE_LEN + 1)),
                ..SatelliteNode::default()
            },
            SatelliteNode {
                downlink_hz: Some(f64::NAN),
                ..SatelliteNode::default()
            },
            SatelliteNode {
                uplink_hz: Some(-1.0),
                ..SatelliteNode::default()
            },
            SatelliteNode {
                transmitter: Some(String::new()),
                ..SatelliteNode::default()
            },
        ];
        for node in bad {
            assert!(!node.valid(), "{node:?}");
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SatelliteFix {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<String>,
    pub ground: SatelliteGround,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ground_track: Vec<LatLon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look: Option<SatelliteLook>,
    #[serde(default)]
    pub visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doppler_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_pass: Option<SatellitePass>,
}

impl SatelliteFix {
    #[must_use]
    pub fn of(status: &SatelliteStatus) -> Option<Self> {
        Some(Self {
            name: status.name.clone(),
            catalog: status.catalog.clone(),
            ground: status.ground?,
            ground_track: status.ground_track.clone(),
            look: status.look,
            visible: status.visible,
            doppler_hz: status.doppler_hz,
            next_pass: status.next_pass,
        })
    }

    fn label(&self) -> Option<String> {
        trimmed(self.name.as_deref()).or_else(|| self.catalog.clone())
    }
}

fn unix_time(seconds: i64) -> Option<FieldValue> {
    jiff::Timestamp::from_second(seconds)
        .ok()
        .map(|at| FieldValue::Time(at.to_string()))
}

impl EventPayload for SatelliteFix {
    fn present(&self, _kind: EventKind) -> Presentation {
        let label = self.label();
        let at = self.ground.at;
        let mut shapes = vec![Shape::Circle {
            center: at,
            radius_m: (self.ground.footprint_km * 1_000.0) as f32,
        }];
        if self.ground_track.len() >= 2 {
            shapes.push(Shape::Line {
                points: self.ground_track.clone(),
                style: LineStyle::Track,
            });
        }
        let summary = match (&label, self.look) {
            (Some(label), Some(look)) => format!("{label} el {:.0}°", look.elevation_deg),
            (Some(label), None) => label.clone(),
            (None, _) => "Satellite".to_owned(),
        };
        Presentation {
            entity: self.catalog.clone().or_else(|| label.clone()),
            label,
            place: Some(Place::at(at).altitude(Some((self.ground.altitude_km * 1_000.0) as f32))),
            shapes,
            fields: fields(&[
                ("Catalog", text(self.catalog.as_deref())),
                (
                    "Altitude",
                    number(Some(self.ground.altitude_km), Unit::Kilometres, 0),
                ),
                (
                    "Footprint",
                    number(Some(self.ground.footprint_km), Unit::Kilometres, 0),
                ),
                ("Visible", flag(Some(self.visible))),
                (
                    "Elevation",
                    number(self.look.map(|look| look.elevation_deg), Unit::Degrees, 1),
                ),
                (
                    "Azimuth",
                    number(self.look.map(|look| look.azimuth_deg), Unit::Degrees, 1),
                ),
                (
                    "Range",
                    number(self.look.map(|look| look.range_km), Unit::Kilometres, 0),
                ),
                ("Doppler", number(self.doppler_hz, Unit::Hz, 0)),
                (
                    "Next pass",
                    self.next_pass.and_then(|pass| unix_time(pass.max_at)),
                ),
            ]),
            ..Presentation::of(summary)
        }
    }
}

#[cfg(test)]
mod fix_tests {
    use super::*;

    fn fix(track: Vec<LatLon>) -> SatelliteFix {
        SatelliteFix {
            name: Some("ISS (ZARYA)".to_owned()),
            catalog: Some("25544".to_owned()),
            ground: SatelliteGround {
                at: LatLon {
                    lat: 51.0,
                    lon: 10.0,
                },
                altitude_km: 420.0,
                footprint_km: 2_200.0,
            },
            ground_track: track,
            look: None,
            visible: true,
            doppler_hz: None,
            next_pass: None,
        }
    }

    #[test]
    fn a_satellite_draws_its_footprint_and_ground_track() {
        let track = vec![
            LatLon {
                lat: 50.0,
                lon: 9.0,
            },
            LatLon {
                lat: 51.0,
                lon: 10.0,
            },
        ];
        let view = fix(track.clone()).present(EventKind::Satellite);

        assert_eq!(view.entity.as_deref(), Some("25544"));
        assert_eq!(view.label.as_deref(), Some("ISS (ZARYA)"));
        assert_eq!(
            view.shapes,
            [
                Shape::Circle {
                    center: LatLon {
                        lat: 51.0,
                        lon: 10.0
                    },
                    radius_m: 2_200_000.0,
                },
                Shape::Line {
                    points: track,
                    style: LineStyle::Track,
                },
            ]
        );
    }

    #[test]
    fn a_satellite_without_a_track_draws_only_its_footprint() {
        assert_eq!(
            fix(Vec::new()).present(EventKind::Satellite).shapes.len(),
            1
        );
    }

    #[test]
    fn a_status_without_a_ground_point_is_no_fix() {
        assert_eq!(SatelliteFix::of(&SatelliteStatus::default()), None);
    }
}

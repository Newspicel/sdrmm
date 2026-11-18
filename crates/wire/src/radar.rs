use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::decode::{EventKind, Symbol};
use crate::geo::{LatLon, bearing_deg, destination, distance_m};
use crate::present::{
    EventPayload, Field, Place, Presentation, Shape, Unit, fields, lat_lon, number, position, text,
    text_of,
};

pub use crate::array::LIGHT_SPEED_M_S;
use crate::array::MAX_ARRAY_LANES;
use crate::processor::reserved_at;

pub const MAX_RADAR_DETECTIONS: usize = 128;
pub const MAX_RADAR_TRACKS: usize = 64;
pub const MAX_RADAR_TENTATIVE: usize = 128;
pub const MAX_TRACK_TRAIL: usize = 16;
pub const MAX_RADAR_TRUTH: usize = 64;
pub const SURFACE_DB_MIN: f32 = -3.0;
pub const SURFACE_DB_MAX: f32 = 30.0;
pub const RADAR_TRACK_EVENT_REPEAT_S: u64 = 5;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RadarAxes {
    pub sample_rate_hz: f64,
    pub carrier_hz: f64,
    pub range_step_m: f32,
    pub gates: u32,
    pub doppler_step_hz: f32,
    pub doppler_rows: u32,
    pub batches: u32,
    pub cpi_ms: f32,
    pub hop_ms: f32,
    pub lanes: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RadarAoa {
    pub azimuth_deg: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearing_deg: Option<f32>,
    pub sigma_deg: f32,
    pub quality: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror_deg: Option<f32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RadarDetection {
    pub range_km: f32,
    pub doppler_hz: f32,
    pub range_rate_mps: f32,
    pub snr_db: f32,
    pub cells: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aoa: Option<RadarAoa>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_id: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrackState {
    #[default]
    Confirmed,
    Coasting,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RadarTrailPoint {
    pub range_km: f32,
    pub doppler_hz: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RadarFix {
    pub lat: f64,
    pub lon: f64,
    pub alt_m: f32,
    #[serde(default)]
    pub alt_from_truth: bool,
    pub major_m: f32,
    pub minor_m: f32,
    pub orientation_deg: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TrackMatch {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callsign: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RadarTrack {
    pub id: u32,
    pub state: TrackState,
    pub range_km: f32,
    pub range_rate_mps: f32,
    pub doppler_hz: f32,
    pub accel_mps2: f32,
    pub range_sigma_m: f32,
    pub rate_sigma_mps: f32,
    pub snr_db: f32,
    pub looks: u32,
    pub misses: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aoa: Option<RadarAoa>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<RadarFix>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truth: Option<TrackMatch>,
    pub trail: Vec<RadarTrailPoint>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TrackTruth {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callsign: Option<String>,
    pub range_km: f32,
    pub doppler_hz: f32,
    pub bearing_deg: f32,
    pub lat: f64,
    pub lon: f64,
    pub altitude_m: f32,
    pub age_s: f32,
    pub in_view: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_id: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceMode {
    #[default]
    Raw,
    Cma,
    DabRemod,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ReferenceHealth {
    pub mode: ReferenceMode,
    pub locked: bool,
    pub quality_db: f32,
    pub fallback_frames: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AoaState {
    #[default]
    Off,
    Ready,
    PhaseUnknown,
    OneLane,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RadarHealth {
    pub suppression_db: Vec<f32>,
    pub unsuppressed_groups: u64,
    pub dropped_samples: u64,
    pub dropped_cpis: u64,
    pub discarded_cpis: u64,
    pub dropped_reports: u64,
    pub lagged_updates: u64,
    pub truncated_detections: u64,
    pub dropped_tracks: u64,
    pub gpu_failures: u64,
    pub load: f32,
    pub front_load: f32,
    pub compute_ms: f32,
    pub noise_floor_db: f32,
    #[serde(default)]
    pub cfar_looks: u32,
    #[serde(default)]
    pub range_correlation: f32,
    pub gpu: bool,
    pub threads: u32,
    pub aoa: AoaState,
    #[serde(default)]
    pub table_out_of_range: bool,
    pub reference: ReferenceHealth,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RadarSite {
    pub lat: f64,
    pub lon: f64,
    pub altitude_m: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RadarGeometry {
    pub receiver: RadarSite,
    pub transmitter: RadarSite,
    pub baseline_km: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_deg: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum RadarProblem {
    NoArray,
    NoTransmitter,
    NoReceiver,
    NoHeading,
    PhaseUnknown,
    TableOutOfRange,
    Overloaded,
    ReferenceLost,
    Refused(String),
}

impl RadarProblem {
    pub const ALL: [Self; 9] = [
        Self::NoArray,
        Self::NoTransmitter,
        Self::NoReceiver,
        Self::NoHeading,
        Self::PhaseUnknown,
        Self::TableOutOfRange,
        Self::Overloaded,
        Self::ReferenceLost,
        Self::Refused(String::new()),
    ];

    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::NoArray => "no_array",
            Self::NoTransmitter => "no_transmitter",
            Self::NoReceiver => "no_receiver",
            Self::NoHeading => "no_heading",
            Self::PhaseUnknown => "phase_unknown",
            Self::TableOutOfRange => "table_out_of_range",
            Self::Overloaded => "overloaded",
            Self::ReferenceLost => "reference_lost",
            Self::Refused(_) => "refused",
        }
    }

    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::NoArray => "No array",
            Self::NoTransmitter => "No transmitter",
            Self::NoReceiver => "No array position",
            Self::NoHeading => "No heading",
            Self::PhaseUnknown => "Phase unknown",
            Self::TableOutOfRange => "Outside cal table",
            Self::Overloaded => "Overloaded",
            Self::ReferenceLost => "Reference lost",
            Self::Refused(text) => text,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RadarUpdate {
    pub seq: u64,
    pub at: String,
    pub axes: RadarAxes,
    pub detections: Vec<RadarDetection>,
    pub tracks: Vec<RadarTrack>,
    pub truth: Vec<TrackTruth>,
    pub health: RadarHealth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<RadarGeometry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<RadarProblem>,
    #[serde(skip)]
    pub events: Vec<RadarTrackEvent>,
}

impl RadarUpdate {
    #[must_use]
    pub fn reserved() -> Self {
        Self {
            at: reserved_at(),
            detections: Vec::with_capacity(MAX_RADAR_DETECTIONS),
            tracks: Vec::with_capacity(MAX_RADAR_TRACKS),
            truth: Vec::with_capacity(MAX_RADAR_TRUTH),
            health: RadarHealth {
                suppression_db: Vec::with_capacity(MAX_ARRAY_LANES as usize),
                ..RadarHealth::default()
            },
            problems: Vec::with_capacity(RadarProblem::ALL.len()),
            events: Vec::with_capacity(MAX_RADAR_TRACKS),
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrackChange {
    Confirmed,
    Update,
    Lost,
}

impl TrackChange {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Update => "update",
            Self::Lost => "lost",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RadarTrackEvent {
    pub track_id: u32,
    pub change: TrackChange,
    pub range_km: f32,
    pub range_rate_mps: f32,
    pub doppler_hz: f32,
    pub snr_db: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearing_deg: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

impl EventPayload for RadarTrackEvent {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            entity: Some(format!("T{}", self.track_id)),
            label: self.target.clone(),
            place: lat_lon(self.lat, self.lon).map(Place::at),
            gone: self.change == TrackChange::Lost,
            fields: self.fields(),
            ..Presentation::of(format!(
                "T{} {} {:.1} km {:+.0} m/s",
                self.track_id,
                self.change.as_str(),
                self.range_km,
                self.range_rate_mps
            ))
        }
    }
}

impl RadarTrackEvent {
    fn fields(&self) -> Vec<Field> {
        fields(&[
            ("track", "Track", text_of(format!("T{}", self.track_id))),
            ("change", "Change", text_of(self.change.as_str())),
            (
                "range",
                "Range",
                number(Some(self.range_km), Unit::Kilometres, 2),
            ),
            (
                "range_rate",
                "Range rate",
                number(Some(self.range_rate_mps), Unit::MetresPerSecond, 0),
            ),
            (
                "doppler",
                "Doppler",
                number(Some(self.doppler_hz), Unit::Hz, 1),
            ),
            ("snr", "SNR", number(Some(self.snr_db), Unit::Db, 1)),
            (
                "bearing",
                "Bearing",
                number(self.bearing_deg, Unit::Degrees, 1),
            ),
            ("position", "Position", position(self.lat, self.lon)),
            ("target", "Target", text(self.target.as_deref())),
        ])
    }
}

impl RadarSite {
    #[must_use]
    pub const fn at(&self) -> LatLon {
        LatLon {
            lat: self.lat,
            lon: self.lon,
        }
    }
}

pub const RADAR_SITES_ENTITY: &str = "sites";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RadarSites {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<RadarGeometry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ranges_km: Vec<f32>,
}

#[must_use]
pub fn bistatic_ring(receiver: LatLon, illuminator: LatLon, range_km: f32) -> Option<Shape> {
    let range_m = f64::from(range_km) * 1_000.0;
    if range_m.is_nan() || range_m <= 0.0 {
        return None;
    }
    let baseline_m = distance_m(receiver, illuminator);
    let axis = bearing_deg(receiver, illuminator);
    Some(Shape::Ellipse {
        center: destination(receiver, axis, baseline_m / 2.0),
        major_m: f64::midpoint(baseline_m, range_m) as f32,
        minor_m: ((range_m * (range_m + 2.0 * baseline_m)).sqrt() / 2.0) as f32,
        bearing_deg: axis as f32,
    })
}

impl EventPayload for RadarSites {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let Some(geometry) = &self.geometry else {
            return Presentation {
                entity: Some(RADAR_SITES_ENTITY.to_owned()),
                gone: true,
                ..Presentation::of("No radar geometry".to_owned())
            };
        };
        let receiver = geometry.receiver.at();
        let illuminator = geometry.transmitter.at();
        let mut shapes = vec![
            Shape::Pin {
                at: receiver,
                symbol: Symbol::ANTENNA,
                label: "Receiver".to_owned(),
            },
            Shape::Pin {
                at: illuminator,
                symbol: Symbol::ANTENNA,
                label: "Illuminator".to_owned(),
            },
        ];
        shapes.extend(
            self.ranges_km
                .iter()
                .filter_map(|range| bistatic_ring(receiver, illuminator, *range)),
        );
        Presentation {
            entity: Some(RADAR_SITES_ENTITY.to_owned()),
            place: Some(Place::at(receiver)),
            shapes,
            fields: fields(&[
                (
                    "receiver",
                    "Receiver",
                    position(Some(receiver.lat), Some(receiver.lon)),
                ),
                (
                    "illuminator",
                    "Illuminator",
                    position(Some(illuminator.lat), Some(illuminator.lon)),
                ),
                (
                    "baseline",
                    "Baseline",
                    number(Some(geometry.baseline_km), Unit::Kilometres, 1),
                ),
                (
                    "echoes",
                    "Echoes",
                    number(Some(self.ranges_km.len() as f64), Unit::None, 0),
                ),
            ]),
            ..Presentation::of(format!(
                "{:.1} km baseline, {} echoes",
                geometry.baseline_km,
                self.ranges_km.len()
            ))
        }
    }
}

#[cfg(test)]
mod tests;

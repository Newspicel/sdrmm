use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    DecodedRecord, DecoderEvent, EventKind, Hue, LatLon, PositionFix, StreamKind, Symbol,
    present::Presentation, rest::CapturedImage,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
pub struct EntityKey {
    pub source: String,
    pub kind: EventKind,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Entity {
    pub key: EntityKey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Symbol>,
    pub hue: Hue,
    pub event: DecoderEvent,
    pub view: Presentation,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trail: Vec<LatLon>,
    pub freq_hz: f64,
    pub first_seen: String,
    pub last_seen: String,
    pub frames: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PositionTrack {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<PositionFix>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trail: Vec<LatLon>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SurfaceRef {
    pub node: String,
    pub kind: StreamKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PropagationCell {
    pub key: String,
    pub latitude: f64,
    pub longitude: f64,
    pub weight: f64,
    pub decodes: u32,
    pub callsigns: u32,
    pub best_freq_hz: f64,
    pub best_snr_db: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured_muf3000_mhz: Option<f64>,
    pub median_distance_km: f64,
    pub last_seen: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PropagationPath {
    pub key: String,
    pub from: LatLon,
    pub to: LatLon,
    pub weight: f64,
    pub freq_hz: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PropagationSummary {
    pub decodes: u32,
    pub grids: u32,
    pub callsigns: u32,
    pub bands: u32,
    pub best_freq_hz: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_muf3000_mhz: Option<f64>,
    pub farthest_km: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PropagationView {
    pub cells: Vec<PropagationCell>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<PropagationPath>,
    pub summary: PropagationSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver: Option<LatLon>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SinkFeed {
    pub entities: Vec<Entity>,
    pub records: Vec<DecodedRecord>,
    pub tracks: Vec<PositionTrack>,
    pub surfaces: Vec<SurfaceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pictures: Vec<CapturedImage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub propagation: Option<PropagationView>,
    pub lost: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SinkUpdate {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub upserts: Vec<Entity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<EntityKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<DecodedRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracks: Option<Vec<PositionTrack>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surfaces: Option<Vec<SurfaceRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pictures: Option<Vec<CapturedImage>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub propagation: Option<PropagationView>,
    pub lost: u64,
}

impl SinkUpdate {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.upserts.is_empty()
            && self.removed.is_empty()
            && self.records.is_empty()
            && self.tracks.is_none()
            && self.surfaces.is_none()
            && self.pictures.is_none()
            && self.propagation.is_none()
    }
}

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use utoipa::ToSchema;

use crate::{
    decode::{EventKind, Symbol},
    geo::LatLon,
    rest::EventAudio,
};

mod aviation;
mod broadcast;
mod datalink;
mod dect;
mod location;
mod maritime;
mod measure;
mod navaid;
mod paging;
mod picture;
mod text;
mod voice;
mod wsjt;

pub(crate) use picture::{picture_fields, picture_size};
pub use wsjt::{grid_lat_lon, square_of};

#[cfg(test)]
mod tests;

const FEET_TO_METRES: f64 = 0.3048;

pub trait EventPayload: Clone + Serialize + DeserializeOwned {
    fn present(&self, kind: EventKind) -> Presentation;

    fn merge(&mut self, newer: &Self) {
        overlay_some(self, newer);
    }

    fn strip_clip(&mut self) {}
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Presentation {
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<Place>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<Shape>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Field>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spot: Option<Spot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearing: Option<BearingRay>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<VoiceFacts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<EventAudio>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip_name: Option<ClipName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callsign: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gone: bool,
}

impl Presentation {
    #[must_use]
    pub fn of(summary: String) -> Self {
        Self {
            summary,
            ..Self::default()
        }
    }
}

#[cfg(test)]
impl Presentation {
    pub(crate) fn field(&self, label: &str) -> Option<&FieldValue> {
        self.fields
            .iter()
            .find(|field| field.label == label)
            .map(|field| &field.value)
    }

    pub(crate) fn labels(&self) -> Vec<&str> {
        self.fields
            .iter()
            .map(|field| field.label.as_str())
            .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ClipName {
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freq_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Place {
    pub at: LatLon,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_deg: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub altitude_m: Option<f32>,
}

impl Place {
    #[must_use]
    pub fn at(at: LatLon) -> Self {
        Self {
            at,
            heading_deg: None,
            altitude_m: None,
        }
    }

    #[must_use]
    pub fn heading(self, heading_deg: Option<f32>) -> Self {
        Self {
            heading_deg,
            ..self
        }
    }

    #[must_use]
    pub fn altitude(self, altitude_m: Option<f32>) -> Self {
        Self {
            altitude_m: altitude_m.filter(|metres| metres.is_finite()),
            ..self
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum Shape {
    Ray {
        from: LatLon,
        bearing_deg: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sigma_deg: Option<f32>,
        length_m: f32,
        faint: bool,
    },
    Ellipse {
        center: LatLon,
        major_m: f32,
        minor_m: f32,
        bearing_deg: f32,
    },
    Circle {
        center: LatLon,
        radius_m: f32,
    },
    Line {
        points: Vec<LatLon>,
        style: LineStyle,
    },
    Pin {
        at: LatLon,
        symbol: Symbol,
        label: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LineStyle {
    Track,
    Forecast,
    Route,
    Footprint,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Field {
    pub label: String,
    pub value: FieldValue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum FieldValue {
    Text(String),
    Number { value: f64, unit: Unit, digits: u8 },
    Flag(bool),
    Time(String),
    Position(LatLon),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    None,
    Hz,
    Db,
    Dbfs,
    Degrees,
    Feet,
    Metres,
    Kilometres,
    Knots,
    MetresPerSecond,
    FeetPerMinute,
    Celsius,
    Percent,
    Hectopascal,
    Seconds,
    Baud,
    Millimetres,
    Volts,
    DbHz,
    KilometresPerHour,
    WattsPerSquareMetre,
    Chips,
    Wpm,
    Dbm,
    Psi,
    Lux,
    Kelvin,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Spot {
    pub grid: String,
    pub at: LatLon,
    pub callsign: String,
    pub snr_db: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BearingRay {
    pub from: LatLon,
    pub true_deg: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sigma_deg: Option<f32>,
    pub confidence: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct VoiceFacts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emergency: Option<bool>,
}

#[must_use]
pub fn trimmed(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

#[must_use]
pub fn bearing(deg: Option<f64>) -> Option<f32> {
    deg.filter(|deg| deg.is_finite())
        .map(|deg| deg.rem_euclid(360.0) as f32)
}

#[must_use]
pub fn lat_lon(lat: Option<f64>, lon: Option<f64>) -> Option<LatLon> {
    let (lat, lon) = lat.zip(lon)?;
    let on_globe = (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon);
    on_globe.then_some(LatLon { lat, lon })
}

#[must_use]
pub fn feet_to_metres(feet: f64) -> f32 {
    (feet * FEET_TO_METRES) as f32
}

#[must_use]
pub fn number<T: Into<f64>>(value: Option<T>, unit: Unit, digits: u8) -> Option<FieldValue> {
    value
        .map(Into::into)
        .filter(|value| value.is_finite())
        .map(|value| FieldValue::Number {
            value,
            unit,
            digits,
        })
}

#[must_use]
pub fn fields(entries: &[(&str, Option<FieldValue>)]) -> Vec<Field> {
    entries
        .iter()
        .filter_map(|(label, value)| {
            value.clone().map(|value| Field {
                label: (*label).to_owned(),
                value,
            })
        })
        .collect()
}

#[must_use]
pub fn text(value: Option<&str>) -> Option<FieldValue> {
    trimmed(value).map(FieldValue::Text)
}

#[must_use]
pub fn text_of(value: impl std::fmt::Display) -> Option<FieldValue> {
    text(Some(&value.to_string()))
}

#[must_use]
pub fn flag(value: Option<bool>) -> Option<FieldValue> {
    value.map(FieldValue::Flag)
}

#[must_use]
pub fn only_if(value: bool) -> Option<FieldValue> {
    value.then_some(FieldValue::Flag(true))
}

#[must_use]
pub fn count<T: Into<f64>>(value: Option<T>) -> Option<FieldValue> {
    number(value, Unit::None, 0)
}

#[must_use]
pub fn repaired(corrected: u32) -> Option<FieldValue> {
    count(Some(corrected).filter(|corrected| *corrected > 0))
}

#[must_use]
pub fn percent(fraction: f32) -> Option<FieldValue> {
    number(Some(fraction * 100.0), Unit::Percent, 0)
}

#[must_use]
pub fn position(lat: Option<f64>, lon: Option<f64>) -> Option<FieldValue> {
    lat_lon(lat, lon).map(FieldValue::Position)
}

#[must_use]
pub fn joined(parts: impl IntoIterator<Item = String>) -> String {
    parts.into_iter().collect::<Vec<_>>().join(" · ")
}

pub fn overlay_some<T: Clone + Serialize + DeserializeOwned>(older: &mut T, newer: &T) {
    *older = overlaid(older, newer).unwrap_or_else(|| newer.clone());
}

fn overlaid<T: Serialize + DeserializeOwned>(older: &T, newer: &T) -> Option<T> {
    let serde_json::Value::Object(mut base) = serde_json::to_value(older).ok()? else {
        return None;
    };
    let serde_json::Value::Object(top) = serde_json::to_value(newer).ok()? else {
        return None;
    };
    for (key, value) in top {
        if !value.is_null() {
            base.insert(key, value);
        }
    }
    serde_json::from_value(serde_json::Value::Object(base)).ok()
}

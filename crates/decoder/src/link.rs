//! The envelope data-link decoders share: a message's type, station, text and place, read from
//! the fields its body declares, with how well the frame decoded.

use sdrmm_wire::present::{
    EventPayload, Field, FieldValue, Place, Presentation, Shape, Unit, count, fields, joined,
    lat_lon, number, position, text, text_of, trimmed,
};
use sdrmm_wire::{EventKind, LatLon};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

const SERIALIZATION_ERROR: &str = "serialization_error";

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Quality {
    pub crc_ok: bool,
    pub fec_corrected: Option<u32>,
    pub snr_db: Option<f32>,
    pub frequency_error_hz: Option<f32>,
}

/// The body fields a decoder keeps its envelope in, each list in priority order. A name is
/// looked for on the body first, then in its nested objects and arrays, depth first.
#[derive(Clone, Copy, Debug)]
pub struct Fields {
    pub kind: &'static [&'static str],
    pub station: &'static [&'static str],
    pub text: &'static [&'static str],
    pub lat: &'static [&'static str],
    pub lon: &'static [&'static str],
}

impl Fields {
    /// The usual names for type, text and place, with the decoder's own station fields.
    #[must_use]
    pub const fn station(station: &'static [&'static str]) -> Self {
        Self {
            kind: &["kind", "name", "type", "label"],
            station,
            text: &["text", "message"],
            lat: &["lat", "latitude"],
            lon: &["lon", "longitude"],
        }
    }

    #[must_use]
    pub const fn text(self, text: &'static [&'static str]) -> Self {
        Self { text, ..self }
    }
}

/// An area a message is addressed to, which the map draws.
pub trait Region: Clone + Serialize + DeserializeOwned {
    fn label(&self) -> String;

    fn center(&self) -> Option<LatLon>;

    fn shapes(&self) -> Vec<Shape>;
}

/// The region of a message addressed to no area.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoRegion {}

impl Region for NoRegion {
    fn label(&self) -> String {
        match *self {}
    }

    fn center(&self) -> Option<LatLon> {
        match *self {}
    }

    fn shapes(&self) -> Vec<Shape> {
        match *self {}
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound(serialize = "R: Serialize", deserialize = "R: DeserializeOwned"))]
pub struct DataLinkMessage<R = NoRegion> {
    pub message_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub crc_ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fec_corrected: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snr_db: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_error_hz: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<R>,
    pub details: Value,
}

impl DataLinkMessage {
    /// This message addressed to `area`.
    #[must_use]
    pub fn in_area<R>(self, area: Option<R>) -> DataLinkMessage<R> {
        DataLinkMessage {
            message_type: self.message_type,
            station: self.station,
            text: self.text,
            crc_ok: self.crc_ok,
            fec_corrected: self.fec_corrected,
            snr_db: self.snr_db,
            frequency_error_hz: self.frequency_error_hz,
            lat: self.lat,
            lon: self.lon,
            raw: self.raw,
            area,
            details: self.details,
        }
    }
}

impl<R: Region> EventPayload for DataLinkMessage<R> {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let area = self.area.as_ref();
        Presentation {
            entity: trimmed(self.station.as_deref()).or_else(|| area.map(R::label)),
            place: lat_lon(self.lat, self.lon)
                .or_else(|| area.and_then(R::center))
                .map(Place::at),
            shapes: area.map_or_default(R::shapes),
            fields: data_link_fields(self),
            verified: Some(self.crc_ok),
            ..Presentation::of(data_link_summary(self))
        }
    }
}

fn data_link_fields<R: Region>(m: &DataLinkMessage<R>) -> Vec<Field> {
    let integrity = if m.crc_ok { "verified" } else { "failed" };
    fields(&[
        ("type", "Type", text(Some(&m.message_type))),
        ("station", "Station", text(m.station.as_deref())),
        ("integrity", "Integrity", text_of(integrity)),
        ("fec_repaired", "FEC repaired", count(m.fec_corrected)),
        ("snr", "SNR", number(m.snr_db, Unit::Db, 1)),
        (
            "frequency_error",
            "Frequency error",
            number(m.frequency_error_hz, Unit::Hz, 1),
        ),
        ("position", "Position", position(m.lat, m.lon)),
        (
            "area",
            "Area",
            m.area.as_ref().and_then(|area| text_of(area.label())),
        ),
        ("text", "Text", text(m.text.as_deref())),
        ("details", "Details", details(&m.details)),
        ("raw", "Raw", text(m.raw.as_deref())),
    ])
}

fn details(details: &Value) -> Option<FieldValue> {
    let empty = details.is_null() || details.as_object().is_some_and(Map::is_empty);
    if empty {
        return None;
    }
    serde_json::to_string_pretty(details)
        .ok()
        .and_then(|json| text(Some(&json)))
}

fn data_link_summary<R>(m: &DataLinkMessage<R>) -> String {
    let mut parts = vec![m.message_type.clone()];
    if let Some(station) = &m.station {
        parts.push(station.clone());
    }
    if let Some(text) = &m.text {
        let text = text.replace('\n', " ");
        if !text.trim().is_empty() {
            parts.push(text.trim().to_owned());
        }
    }
    joined(parts)
}

/// `body` in the envelope, its type, station, text and place read from the `fields` it
/// declares. A body that will not serialize comes back failed, never as decoded.
pub fn message<T: Serialize + ?Sized>(
    body: &T,
    fields: &Fields,
    quality: Quality,
    raw: Option<&[u8]>,
) -> DataLinkMessage {
    let raw = raw.map(sdrmm_wire::phone::hex);
    let body = match serde_json::to_value(body) {
        Ok(value) => value,
        Err(error) => return damaged(quality, raw, &error),
    };
    DataLinkMessage {
        message_type: find_string(&body, fields.kind).unwrap_or_else(|| "frame".to_owned()),
        station: find_string(&body, fields.station),
        text: find_string(&body, fields.text),
        crc_ok: quality.crc_ok,
        fec_corrected: quality.fec_corrected,
        snr_db: quality.snr_db,
        frequency_error_hz: quality.frequency_error_hz,
        lat: find_number(&body, fields.lat),
        lon: find_number(&body, fields.lon),
        raw,
        area: None,
        details: body,
    }
}

fn damaged(quality: Quality, raw: Option<String>, error: &serde_json::Error) -> DataLinkMessage {
    DataLinkMessage {
        message_type: SERIALIZATION_ERROR.to_owned(),
        station: None,
        text: Some(error.to_string()),
        crc_ok: false,
        fec_corrected: quality.fec_corrected,
        snr_db: quality.snr_db,
        frequency_error_hz: quality.frequency_error_hz,
        lat: None,
        lon: None,
        raw,
        area: None,
        details: Value::Object(Map::from_iter([(
            SERIALIZATION_ERROR.to_owned(),
            Value::String(error.to_string()),
        )])),
    }
}

/// The data-link message `event` carries, when it is of kind `tag`.
#[must_use]
pub fn read(event: &sdrmm_wire::DecoderEvent, tag: &'static str) -> Option<DataLinkMessage> {
    if event.kind == EventKind::named(tag) {
        event.payload()
    } else {
        None
    }
}

fn find_string(value: &Value, names: &[&str]) -> Option<String> {
    match value {
        Value::Object(object) => {
            for name in names {
                match object.get(*name) {
                    Some(Value::String(found)) if !found.is_empty() => return Some(found.clone()),
                    Some(Value::Number(found)) => return Some(found.to_string()),
                    _ => {}
                }
            }
            object.values().find_map(|value| find_string(value, names))
        }
        Value::Array(items) => items.iter().find_map(|item| find_string(item, names)),
        _ => None,
    }
}

fn find_number(value: &Value, names: &[&str]) -> Option<f64> {
    match value {
        Value::Object(object) => {
            for name in names {
                if let Some(found) = object.get(*name).and_then(Value::as_f64) {
                    return Some(found);
                }
            }
            object.values().find_map(|value| find_number(value, names))
        }
        Value::Array(items) => items.iter().find_map(|item| find_number(item, names)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

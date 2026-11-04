use sdrmm_wire::{Capability, Hue, KindSpec, Symbol};
use serde::{Deserialize, Serialize};

use crate::inmarsat_stdc::SeaArea;
use sdrmm_wire::EventKind;

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::FieldValue;
use sdrmm_wire::present::Place;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::count;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::joined;
use sdrmm_wire::present::lat_lon;
use sdrmm_wire::present::number;
use sdrmm_wire::present::position;
use sdrmm_wire::present::text;
use sdrmm_wire::present::text_of;
use sdrmm_wire::present::trimmed;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DataLinkMessage {
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
    pub area: Option<crate::inmarsat_stdc::SeaArea>,
    pub details: serde_json::Value,
}

impl EventPayload for DataLinkMessage {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let area = self.area.as_ref();
        Presentation {
            entity: trimmed(self.station.as_deref()).or_else(|| area.map(SeaArea::label)),
            place: lat_lon(self.lat, self.lon)
                .or_else(|| area.and_then(SeaArea::center))
                .map(Place::at),
            shapes: area.map(SeaArea::shapes).unwrap_or_default(),
            fields: data_link_fields(self),
            verified: Some(self.crc_ok),
            ..Presentation::of(data_link_summary(self))
        }
    }
}

fn data_link_fields(m: &DataLinkMessage) -> Vec<Field> {
    let integrity = if m.crc_ok { "verified" } else { "failed" };
    fields(&[
        ("Type", text(Some(&m.message_type))),
        ("Station", text(m.station.as_deref())),
        ("Integrity", text_of(integrity)),
        ("FEC repaired", count(m.fec_corrected)),
        ("SNR", number(m.snr_db, Unit::Db, 1)),
        ("Frequency error", number(m.frequency_error_hz, Unit::Hz, 1)),
        ("Position", position(m.lat, m.lon)),
        (
            "Area",
            m.area.as_ref().and_then(|area| text_of(area.label())),
        ),
        ("Text", text(m.text.as_deref())),
        ("Details", details(&m.details)),
        ("Raw", text(m.raw.as_deref())),
    ])
}

fn details(details: &serde_json::Value) -> Option<FieldValue> {
    let empty = details.is_null() || details.as_object().is_some_and(serde_json::Map::is_empty);
    if empty {
        return None;
    }
    serde_json::to_string_pretty(details)
        .ok()
        .and_then(|json| text(Some(&json)))
}

fn data_link_summary(m: &DataLinkMessage) -> String {
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

pub(crate) fn dsc_kind() -> KindSpec {
    KindSpec::tagged::<DataLinkMessage>(EventKind::named("dsc"), "DSC", "Vessels", Hue::Amber)
        .caps(&[Capability::Log, Capability::Station, Capability::Geo])
        .symbol(Symbol::Ship)
        .sighting()
        .build()
}

pub(crate) fn inmarsat_stdc_kind() -> KindSpec {
    KindSpec::tagged::<DataLinkMessage>(
        EventKind::named("inmarsat_stdc"),
        "Inmarsat STD-C",
        "Vessels",
        Hue::Amber,
    )
    .caps(&[Capability::Log, Capability::Station, Capability::Geo])
    .symbol(Symbol::Ship)
    .sighting()
    .build()
}

pub(crate) fn inmarsat_aero_kind() -> KindSpec {
    KindSpec::tagged::<DataLinkMessage>(
        EventKind::named("inmarsat_aero"),
        "Inmarsat Aero",
        "Aircraft",
        Hue::Teal,
    )
    .caps(&[Capability::Log, Capability::Station, Capability::Geo])
    .symbol(Symbol::Plane)
    .sighting()
    .build()
}

pub(crate) fn vdl2_kind() -> KindSpec {
    KindSpec::tagged::<DataLinkMessage>(
        EventKind::named("vdl2"),
        "VDL Mode 2",
        "Aircraft",
        Hue::Teal,
    )
    .caps(&[Capability::Log, Capability::Station, Capability::Geo])
    .symbol(Symbol::Plane)
    .sighting()
    .build()
}

pub(crate) fn hfdl_kind() -> KindSpec {
    KindSpec::tagged::<DataLinkMessage>(EventKind::named("hfdl"), "HFDL", "Aircraft", Hue::Teal)
        .caps(&[Capability::Log, Capability::Station, Capability::Geo])
        .symbol(Symbol::Plane)
        .sighting()
        .build()
}

pub(crate) fn iridium_kind() -> KindSpec {
    KindSpec::tagged::<DataLinkMessage>(EventKind::named("iridium"), "Iridium", "Bursts", Hue::Blue)
        .caps(&[Capability::Log, Capability::Station, Capability::Geo])
        .symbol(Symbol::Dot)
        .sighting()
        .build()
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::LatLon;

    #[test]
    fn every_data_link_kind_shows_quality_place_and_text() {
        let view = crate::datalink::sample(serde_json::json!({ "level": 3 }))
            .present(&EventKind::named("hfdl"));
        assert_eq!(
            view.labels(),
            [
                "Type",
                "Station",
                "Integrity",
                "FEC repaired",
                "SNR",
                "Frequency error",
                "Position",
                "Text",
                "Details"
            ]
        );
        assert_eq!(view.field("Station"), text(Some(".D-AIBC")).as_ref());
        assert_eq!(view.field("Integrity"), text(Some("verified")).as_ref());
        assert_eq!(view.field("SNR"), number(Some(12.25), Unit::Db, 1).as_ref());
        assert_eq!(
            view.field("Frequency error"),
            number(Some(-41.5), Unit::Hz, 1).as_ref()
        );
        assert_eq!(
            view.field("Position"),
            Some(&FieldValue::Position(LatLon {
                lat: 52.5,
                lon: 13.4
            }))
        );
        assert_eq!(view.field("Text"), text(Some("FL370")).as_ref());
        assert_eq!(
            view.field("Details"),
            text(Some("{\n  \"level\": 3\n}")).as_ref()
        );
    }

    #[test]
    fn empty_details_show_nothing() {
        let view = crate::datalink::sample(serde_json::json!({})).present(&EventKind::named("dsc"));
        assert_eq!(view.field("Details"), None);
    }
}

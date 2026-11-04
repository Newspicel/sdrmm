use sdrmm_wire::{Capability, EventContract, Hue, KindSpec, Symbol};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::Place;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::joined;
use sdrmm_wire::present::lat_lon;
use sdrmm_wire::present::only_if;
use sdrmm_wire::present::position;
use sdrmm_wire::present::text;
use sdrmm_wire::present::text_of;
use sdrmm_wire::present::trimmed;

fn default_acars_bandwidth_hz() -> f64 {
    12_500.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AcarsParams {
    #[serde(default = "default_acars_bandwidth_hz")]
    pub bandwidth_hz: f64,
}

impl Default for AcarsParams {
    fn default() -> Self {
        Self {
            bandwidth_hz: default_acars_bandwidth_hz(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AcarsMessage {
    pub mode: char,
    pub registration: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ack: Option<char>,
    pub label: String,
    pub block_id: char,
    pub downlink: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq_no: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flight: Option<String>,
    pub text: String,
    pub more: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sublabel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mfi: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assstat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoded: Option<serde_json::Value>,
}

impl EventPayload for AcarsMessage {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            label: trimmed(self.flight.as_deref()),
            entity: Some(self.registration.clone()),
            place: lat_lon(self.lat, self.lon).map(Place::at),
            fields: acars_fields(self),
            ..Presentation::of(acars_summary(self))
        }
    }
}

fn acars_fields(a: &AcarsMessage) -> Vec<Field> {
    let direction = if a.downlink { "downlink" } else { "uplink" };
    fields(&[
        ("Registration", text(Some(&a.registration))),
        ("Flight", text(a.flight.as_deref())),
        ("Label", text(Some(&a.label))),
        ("Sublabel", text(a.sublabel.as_deref())),
        ("MFI", text(a.mfi.as_deref())),
        ("Mode", text_of(a.mode)),
        ("Block", text_of(a.block_id)),
        ("Direction", text_of(direction)),
        ("Sequence", text(a.seq_no.as_deref())),
        ("Ack", a.ack.map_or_else(|| text_of("NAK"), text_of)),
        ("Continues", only_if(a.more)),
        ("Reassembly", text(a.assstat.as_deref())),
        ("Position", position(a.lat, a.lon)),
        ("Text", text(Some(&a.text))),
    ])
}

fn acars_summary(a: &AcarsMessage) -> String {
    let mut parts = vec![a.registration.clone()];
    if let Some(flight) = &a.flight {
        parts.push(flight.trim().to_owned());
    }
    parts.push(format!("[{}]", a.label));
    let text = a.text.replace('\n', " ");
    let text = text.trim();
    if !text.is_empty() {
        parts.push(text.to_owned());
    }
    joined(parts)
}

pub(crate) fn acars_kind() -> KindSpec {
    KindSpec::of::<AcarsMessage>("ACARS", "Aircraft", Hue::Teal)
        .caps(&[Capability::Log, Capability::Station, Capability::Geo])
        .symbol(Symbol::Plane)
        .sighting()
        .build()
}

impl EventContract for AcarsMessage {
    const KIND: EventKind = EventKind::named("acars");
}

impl sdrmm_wire::DecoderSettings for AcarsParams {
    const TYPE_ID: &'static str = "acars";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::present::FieldValue;

    #[test]
    fn acars_names_direction_ack_and_continuation() {
        let message: AcarsMessage = serde_json::from_value(serde_json::json!({
            "mode": "2", "registration": "D-AIBC", "flight": "LH0400 ", "label": "H1",
            "block_id": "3", "downlink": true, "seq_no": "M01A",
            "text": "POS N52.5 E013.4\nFL370", "more": true,
        }))
        .unwrap();
        let view = message.present(&EventKind::named("acars"));
        assert_eq!(view.field("Flight"), text(Some("LH0400")).as_ref());
        assert_eq!(view.field("Direction"), text(Some("downlink")).as_ref());
        assert_eq!(view.field("Sequence"), text(Some("M01A")).as_ref());
        assert_eq!(view.field("Ack"), text(Some("NAK")).as_ref());
        assert_eq!(view.field("Continues"), Some(&FieldValue::Flag(true)));
        assert_eq!(
            view.field("Text"),
            text(Some("POS N52.5 E013.4\nFL370")).as_ref()
        );
    }
}

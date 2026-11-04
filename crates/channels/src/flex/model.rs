use sdrmm_wire::{Capability, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::count;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::flag;
use sdrmm_wire::present::repaired;
use sdrmm_wire::present::text;
use sdrmm_wire::present::text_of;

pub(crate) fn default_pager_bandwidth_hz() -> f64 {
    12_500.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlexParams {
    #[serde(default = "default_pager_bandwidth_hz")]
    pub bandwidth_hz: f64,
    #[serde(default)]
    pub invert: bool,
}

impl Default for FlexParams {
    fn default() -> Self {
        Self {
            bandwidth_hz: default_pager_bandwidth_hz(),
            invert: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PagerPayload {
    Tone,
    Numeric,
    Alpha,
    Binary,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlexMessage {
    pub address: u64,
    pub payload: PagerPayload,
    pub text: String,
    pub baud: u16,
    pub levels: u8,
    pub cycle: u8,
    pub frame: u8,
    pub phase: char,
    pub errors_corrected: u32,
    /// False when a fragment of a page split over frames never arrived.
    #[serde(default = "complete")]
    pub complete: bool,
}

fn complete() -> bool {
    true
}

pub(crate) fn pager_payload(payload: PagerPayload) -> &'static str {
    match payload {
        PagerPayload::Tone => "tone",
        PagerPayload::Numeric => "numeric",
        PagerPayload::Alpha => "alpha",
        PagerPayload::Binary => "binary",
    }
}

impl EventPayload for FlexMessage {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let summary = if self.text.is_empty() {
            format!("{} · {:?}", self.address, self.payload)
        } else {
            format!("{}: {}", self.address, self.text)
        };
        let summary = if self.complete {
            summary
        } else {
            format!("{summary} …")
        };
        Presentation {
            entity: Some(self.address.to_string()),
            fields: flex_fields(self),
            ..Presentation::of(summary)
        }
    }
}

fn flex_fields(p: &FlexMessage) -> Vec<Field> {
    fields(&[
        ("Address", text_of(p.address)),
        ("Payload", text_of(pager_payload(p.payload))),
        ("Mode", text_of(format!("{}/{}", p.baud, p.levels))),
        ("Cycle", count(Some(p.cycle))),
        ("Frame", count(Some(p.frame))),
        ("Phase", text_of(p.phase)),
        (
            "Complete",
            flag(Some(p.complete).filter(|complete| !complete)),
        ),
        ("Repaired", repaired(p.errors_corrected)),
        ("Text", text(Some(&p.text))),
    ])
}

pub(crate) fn flex_kind() -> KindSpec {
    KindSpec::of::<FlexMessage>("FLEX", "Pagers", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station])
        .sighting()
        .build()
}

impl EventContract for FlexMessage {
    const KIND: EventKind = EventKind::named("flex");
}

impl sdrmm_wire::DecoderSettings for FlexParams {
    const TYPE_ID: &'static str = "flex";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use crate::ermes::ErmesMessage;

    fn present<T: EventPayload>(kind: EventKind, json: serde_json::Value) -> Presentation {
        serde_json::from_value::<T>(json).unwrap().present(&kind)
    }

    #[test]
    fn flex_and_ermes_list_their_frame_facts() {
        let flex = present::<FlexMessage>(
            EventKind::named("flex"),
            serde_json::json!({
                "address": 1234567, "payload": "alpha", "text": "HELLO", "baud": 1600,
                "levels": 2, "cycle": 3, "frame": 17, "phase": "a", "errors_corrected": 0,
            }),
        );
        assert_eq!(flex.field("Mode"), text(Some("1600/2")).as_ref());
        assert_eq!(flex.field("Phase"), text(Some("a")).as_ref());
        assert_eq!(flex.field("Repaired"), None);
        assert_eq!(flex.field("Complete"), None);
        let partial = present::<FlexMessage>(
            EventKind::named("flex"),
            serde_json::json!({
                "address": 1234567, "payload": "alpha", "text": "HEL", "baud": 1600,
                "levels": 2, "cycle": 3, "frame": 17, "phase": "a", "errors_corrected": 0,
                "complete": false,
            }),
        );
        assert_eq!(partial.field("Complete"), flag(Some(false)).as_ref());
        assert_eq!(partial.summary, "1234567: HEL …");
        let ermes = present::<ErmesMessage>(
            EventKind::named("ermes"),
            serde_json::json!({
                "local_address": 42, "message_number": 5, "payload": "numeric", "text": "123",
                "urgent": true, "alert": 2, "errors_corrected": 1,
            }),
        );
        assert_eq!(ermes.field("Urgent"), flag(Some(true)).as_ref());
        assert_eq!(ermes.field("Payload"), text(Some("numeric")).as_ref());
    }
}

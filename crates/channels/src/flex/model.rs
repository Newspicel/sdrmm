use sdrmm_wire::{
    Capability, EventContract, Hue, KindSpec, PagerPayload, default_pager_bandwidth_hz,
};
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
        ("address", "Address", text_of(p.address)),
        ("payload", "Payload", text_of(p.payload.label())),
        ("mode", "Mode", text_of(format!("{}/{}", p.baud, p.levels))),
        ("cycle", "Cycle", count(Some(p.cycle))),
        ("frame", "Frame", count(Some(p.frame))),
        ("phase", "Phase", text_of(p.phase)),
        (
            "complete",
            "Complete",
            flag(Some(p.complete).filter(|complete| !complete)),
        ),
        ("repaired", "Repaired", repaired(p.errors_corrected)),
        ("text", "Text", text(Some(&p.text))),
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

    fn present<T: EventPayload>(kind: &EventKind, json: serde_json::Value) -> Presentation {
        serde_json::from_value::<T>(json).unwrap().present(kind)
    }

    #[test]
    fn flex_lists_its_frame_facts() {
        let flex = present::<FlexMessage>(
            &EventKind::named("flex"),
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
            &EventKind::named("flex"),
            serde_json::json!({
                "address": 1234567, "payload": "alpha", "text": "HEL", "baud": 1600,
                "levels": 2, "cycle": 3, "frame": 17, "phase": "a", "errors_corrected": 0,
                "complete": false,
            }),
        );
        assert_eq!(partial.field("Complete"), flag(Some(false)).as_ref());
        assert_eq!(partial.summary, "1234567: HEL …");
    }
}

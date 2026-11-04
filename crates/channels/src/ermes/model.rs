use sdrmm_wire::{Capability, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use crate::flex::PagerPayload;
use crate::flex::default_pager_bandwidth_hz;
use crate::flex::pager_payload;
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
pub struct ErmesParams {
    #[serde(default = "default_pager_bandwidth_hz")]
    pub bandwidth_hz: f64,
    #[serde(default)]
    pub invert: bool,
}

impl Default for ErmesParams {
    fn default() -> Self {
        Self {
            bandwidth_hz: default_pager_bandwidth_hz(),
            invert: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErmesMessage {
    pub local_address: u32,
    pub message_number: u8,
    pub payload: PagerPayload,
    pub text: String,
    pub urgent: bool,
    pub alert: u8,
    pub errors_corrected: u32,
}

impl EventPayload for ErmesMessage {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let summary = if self.text.is_empty() {
            format!("{} · {:?}", self.local_address, self.payload)
        } else {
            format!("{}: {}", self.local_address, self.text)
        };
        Presentation {
            entity: Some(self.local_address.to_string()),
            fields: ermes_fields(self),
            ..Presentation::of(summary)
        }
    }
}

fn ermes_fields(p: &ErmesMessage) -> Vec<Field> {
    fields(&[
        ("address", "Address", text_of(p.local_address)),
        ("number", "Number", count(Some(p.message_number))),
        ("payload", "Payload", text_of(pager_payload(p.payload))),
        ("urgent", "Urgent", flag(Some(p.urgent))),
        ("alert", "Alert", count(Some(p.alert))),
        ("repaired", "Repaired", repaired(p.errors_corrected)),
        ("text", "Text", text(Some(&p.text))),
    ])
}

pub(crate) fn ermes_kind() -> KindSpec {
    KindSpec::of::<ErmesMessage>("ERMES", "Pagers", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station])
        .sighting()
        .build()
}

impl EventContract for ErmesMessage {
    const KIND: EventKind = EventKind::named("ermes");
}

impl sdrmm_wire::DecoderSettings for ErmesParams {
    const TYPE_ID: &'static str = "ermes";
}

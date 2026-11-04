use sdrmm_wire::{Capability, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::labelled;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::number;
use sdrmm_wire::present::repaired;
use sdrmm_wire::present::text;
use sdrmm_wire::present::text_of;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PocsagBaud {
    #[default]
    Auto,
    B512,
    B1200,
    B2400,
}

impl PocsagBaud {
    #[must_use]
    pub fn rates(self) -> &'static [u16] {
        match self {
            Self::Auto => &[2_400, 1_200, 512],
            Self::B512 => &[512],
            Self::B1200 => &[1_200],
            Self::B2400 => &[2_400],
        }
    }
}

fn default_pocsag_bandwidth_hz() -> f64 {
    12_500.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PocsagParams {
    #[serde(default)]
    pub baud: PocsagBaud,
    #[serde(default = "default_pocsag_bandwidth_hz")]
    pub bandwidth_hz: f64,
    #[serde(default)]
    pub invert: bool,
}

impl Default for PocsagParams {
    fn default() -> Self {
        Self {
            baud: PocsagBaud::default(),
            bandwidth_hz: default_pocsag_bandwidth_hz(),
            invert: false,
        }
    }
}

labelled! {
    PocsagBaud { Auto => "Auto", B512 => "512", B1200 => "1200", B2400 => "2400" }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PocsagPayload {
    Tone,
    Numeric,
    Alpha,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PocsagMessage {
    pub address: u32,
    pub function: u8,
    pub baud: u16,
    pub payload: PocsagPayload,
    pub text: String,
    pub errors_corrected: u32,
}

const FUNCTION_LETTERS: [char; 4] = ['A', 'B', 'C', 'D'];

impl EventPayload for PocsagMessage {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let summary = if self.text.is_empty() {
            format!("{} ({})", self.address, self.function)
        } else {
            format!("{}: {}", self.address, self.text)
        };
        Presentation {
            entity: Some(self.address.to_string()),
            fields: pocsag_fields(self),
            ..Presentation::of(summary)
        }
    }
}

fn pocsag_fields(p: &PocsagMessage) -> Vec<Field> {
    let letter = FUNCTION_LETTERS
        .get(usize::from(p.function))
        .map_or_else(|| p.function.to_string(), char::to_string);
    fields(&[
        ("RIC", text_of(format!("{:07}", p.address))),
        ("Function", text_of(format!("{letter} ({})", p.function))),
        ("Baud", number(Some(p.baud), Unit::Baud, 0)),
        ("Payload", text_of(pocsag_payload(p.payload))),
        ("Repaired", repaired(p.errors_corrected)),
        ("Text", text(Some(&p.text))),
    ])
}

fn pocsag_payload(payload: PocsagPayload) -> &'static str {
    match payload {
        PocsagPayload::Tone => "tone",
        PocsagPayload::Numeric => "numeric",
        PocsagPayload::Alpha => "alpha",
    }
}

pub(crate) fn pocsag_kind() -> KindSpec {
    KindSpec::of::<PocsagMessage>("POCSAG", "Pagers", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station])
        .sighting()
        .build()
}

impl EventContract for PocsagMessage {
    const KIND: EventKind = EventKind::named("pocsag");
}

impl sdrmm_wire::DecoderSettings for PocsagParams {
    const TYPE_ID: &'static str = "pocsag";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::DecoderEvent;
    use sdrmm_wire::present::count;

    fn present<T: EventPayload>(kind: EventKind, json: serde_json::Value) -> Presentation {
        serde_json::from_value::<T>(json).unwrap().present(&kind)
    }

    #[test]
    fn decoder_event_is_adjacently_tagged() {
        let ev = DecoderEvent::of(&PocsagMessage {
            address: 1_234_567,
            function: 3,
            baud: 1_200,
            payload: PocsagPayload::Alpha,
            text: "TEST".to_owned(),
            errors_corrected: 1,
        });
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["kind"], "pocsag");
        assert_eq!(json["data"]["address"], 1_234_567);
        assert_eq!(json["data"]["payload"], "alpha");
        assert_eq!(ev.kind(), EventKind::named("pocsag"));

        let back: DecoderEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, ev);
    }

    #[test]
    fn pocsag_pads_the_ric_and_names_the_function_bit() {
        let view = present::<PocsagMessage>(
            EventKind::named("pocsag"),
            serde_json::json!({
                "address": 1234, "function": 3, "baud": 1200, "payload": "alpha",
                "text": "CALL 42", "errors_corrected": 2,
            }),
        );
        assert_eq!(
            view.labels(),
            ["RIC", "Function", "Baud", "Payload", "Repaired", "Text"]
        );
        assert_eq!(view.field("RIC"), text(Some("0001234")).as_ref());
        assert_eq!(view.field("Function"), text(Some("D (3)")).as_ref());
        assert_eq!(
            view.field("Baud"),
            number(Some(1200), Unit::Baud, 0).as_ref()
        );
        assert_eq!(view.field("Repaired"), count(Some(2)).as_ref());
        assert_eq!(view.field("Text"), text(Some("CALL 42")).as_ref());
    }

    #[test]
    fn a_tone_only_page_has_no_text_or_repairs() {
        let view = present::<PocsagMessage>(
            EventKind::named("pocsag"),
            serde_json::json!({
                "address": 7, "function": 0, "baud": 512, "payload": "tone",
                "text": "", "errors_corrected": 0,
            }),
        );
        assert_eq!(view.labels(), ["RIC", "Function", "Baud", "Payload"]);
    }
}

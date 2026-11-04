use sdrmm_wire::{Capability, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::named;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::number;
use sdrmm_wire::present::text;
use sdrmm_wire::present::text_of;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelcallSystem {
    #[default]
    Ccir1,
    Zvei1,
}

impl SelcallSystem {
    pub const ALL: [Self; 2] = [Self::Ccir1, Self::Zvei1];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ccir1 => "CCIR-1",
            Self::Zvei1 => "ZVEI-1",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelcallParams {
    #[serde(default)]
    pub system: SelcallSystem,
}

named!(SelcallSystem);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelcallSequence {
    pub system: crate::selcall::SelcallSystem,
    pub code: String,
    pub tone_ms: u32,
}

impl EventPayload for SelcallSequence {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let system = self.system.label();
        Presentation {
            fields: fields(&[
                ("Plan", text_of(system)),
                ("Code", text(Some(&self.code))),
                (
                    "Tone",
                    number(Some(f64::from(self.tone_ms) / 1_000.0), Unit::Seconds, 3),
                ),
            ]),
            ..Presentation::of(format!("{system} · {}", self.code))
        }
    }
}

pub(crate) fn selcall_kind() -> KindSpec {
    KindSpec::of::<SelcallSequence>("Selcall", "Selcall", Hue::Muted)
        .caps(&[Capability::Log])
        .build()
}

impl EventContract for SelcallSequence {
    const KIND: EventKind = EventKind::named("selcall");
}

impl sdrmm_wire::DecoderSettings for SelcallParams {
    const TYPE_ID: &'static str = "selcall";
}

#[cfg(test)]
mod model_tests {
    use sdrmm_wire::present::FieldValue;

    use super::*;

    fn field<'a>(view: &'a Presentation, label: &str) -> Option<&'a FieldValue> {
        view.fields
            .iter()
            .find(|field| field.label == label)
            .map(|field| &field.value)
    }

    fn present<T: EventPayload + EventContract>(json: serde_json::Value) -> Presentation {
        serde_json::from_value::<T>(json)
            .expect("payload parses")
            .present(&T::KIND)
    }

    #[test]
    fn selcall_shows_the_plan_code_and_tone_length() {
        let view = present::<SelcallSequence>(
            serde_json::json!({ "system": "zvei1", "code": "A11D0", "tone_ms": 70 }),
        );
        assert_eq!(
            view.fields
                .iter()
                .map(|field| field.label.as_str())
                .collect::<Vec<_>>(),
            ["Plan", "Code", "Tone"]
        );
        assert_eq!(field(&view, "Plan"), text(Some("ZVEI-1")).as_ref());
        assert_eq!(
            field(&view, "Tone"),
            number(Some(0.07), Unit::Seconds, 3).as_ref()
        );
    }
}

use super::{
    EventPayload, Field, Presentation, Unit, count, fields, flag, number, repaired, text, text_of,
};
use crate::decode::{
    ErmesMessage, EventKind, FlexMessage, PagerPayload, PocsagMessage, PocsagPayload,
    SelcallSequence,
};

const FUNCTION_LETTERS: [char; 4] = ['A', 'B', 'C', 'D'];

impl EventPayload for PocsagMessage {
    fn present(&self, _kind: EventKind) -> Presentation {
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

fn pager_payload(payload: PagerPayload) -> &'static str {
    match payload {
        PagerPayload::Tone => "tone",
        PagerPayload::Numeric => "numeric",
        PagerPayload::Alpha => "alpha",
        PagerPayload::Binary => "binary",
    }
}

impl EventPayload for FlexMessage {
    fn present(&self, _kind: EventKind) -> Presentation {
        let summary = if self.text.is_empty() {
            format!("{} · {:?}", self.address, self.payload)
        } else {
            format!("{}: {}", self.address, self.text)
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
        ("Repaired", repaired(p.errors_corrected)),
        ("Text", text(Some(&p.text))),
    ])
}

impl EventPayload for ErmesMessage {
    fn present(&self, _kind: EventKind) -> Presentation {
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
        ("Address", text_of(p.local_address)),
        ("Number", count(Some(p.message_number))),
        ("Payload", text_of(pager_payload(p.payload))),
        ("Urgent", flag(Some(p.urgent))),
        ("Alert", count(Some(p.alert))),
        ("Repaired", repaired(p.errors_corrected)),
        ("Text", text(Some(&p.text))),
    ])
}

impl EventPayload for SelcallSequence {
    fn present(&self, _kind: EventKind) -> Presentation {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn present<T: EventPayload>(kind: EventKind, json: serde_json::Value) -> Presentation {
        serde_json::from_value::<T>(json).unwrap().present(kind)
    }

    #[test]
    fn pocsag_pads_the_ric_and_names_the_function_bit() {
        let view = present::<PocsagMessage>(
            EventKind::Pocsag,
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
            EventKind::Pocsag,
            serde_json::json!({
                "address": 7, "function": 0, "baud": 512, "payload": "tone",
                "text": "", "errors_corrected": 0,
            }),
        );
        assert_eq!(view.labels(), ["RIC", "Function", "Baud", "Payload"]);
    }

    #[test]
    fn flex_and_ermes_list_their_frame_facts() {
        let flex = present::<FlexMessage>(
            EventKind::Flex,
            serde_json::json!({
                "address": 1234567, "payload": "alpha", "text": "HELLO", "baud": 1600,
                "levels": 2, "cycle": 3, "frame": 17, "phase": "a", "errors_corrected": 0,
            }),
        );
        assert_eq!(flex.field("Mode"), text(Some("1600/2")).as_ref());
        assert_eq!(flex.field("Phase"), text(Some("a")).as_ref());
        assert_eq!(flex.field("Repaired"), None);
        let ermes = present::<ErmesMessage>(
            EventKind::Ermes,
            serde_json::json!({
                "local_address": 42, "message_number": 5, "payload": "numeric", "text": "123",
                "urgent": true, "alert": 2, "errors_corrected": 1,
            }),
        );
        assert_eq!(ermes.field("Urgent"), flag(Some(true)).as_ref());
        assert_eq!(ermes.field("Payload"), text(Some("numeric")).as_ref());
    }

    #[test]
    fn selcall_shows_the_plan_code_and_tone_length() {
        let view = present::<SelcallSequence>(
            EventKind::Selcall,
            serde_json::json!({ "system": "zvei1", "code": "A11D0", "tone_ms": 70 }),
        );
        assert_eq!(view.labels(), ["Plan", "Code", "Tone"]);
        assert_eq!(view.field("Plan"), text(Some("ZVEI-1")).as_ref());
        assert_eq!(
            view.field("Tone"),
            number(Some(0.07), Unit::Seconds, 3).as_ref()
        );
    }
}

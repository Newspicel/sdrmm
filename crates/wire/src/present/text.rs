use super::{EventPayload, Field, Presentation, Unit, fields, number, text, text_of};
use crate::decode::{CwSkimmerSpot, EventKind, MorseText, PskText, RttyText};

fn text_pane(text: &str, fields: Vec<Field>) -> Presentation {
    Presentation {
        text: Some(text.to_owned()),
        fields,
        ..Presentation::of(text.to_owned())
    }
}

impl EventPayload for RttyText {
    fn present(&self, _kind: EventKind) -> Presentation {
        text_pane(&self.text, Vec::new())
    }
}

impl EventPayload for MorseText {
    fn present(&self, _kind: EventKind) -> Presentation {
        text_pane(
            &self.text,
            fields(&[("Speed", number(Some(self.wpm), Unit::Wpm, 0))]),
        )
    }
}

impl EventPayload for PskText {
    fn present(&self, _kind: EventKind) -> Presentation {
        text_pane(&self.text, fields(&[("Mode", text_of(self.baud.label()))]))
    }
}

impl EventPayload for CwSkimmerSpot {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            fields: fields(&[
                ("Offset", number(Some(self.offset_hz), Unit::Hz, 0)),
                ("Speed", number(Some(self.wpm), Unit::Wpm, 0)),
                ("SNR", number(Some(self.snr_db), Unit::Db, 1)),
                ("Text", text(Some(&self.text))),
            ]),
            ..Presentation::of(format!(
                "{:+.0} Hz · {:.0} WPM · {}",
                self.offset_hz, self.wpm, self.text
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PskBaud;

    #[test]
    fn rtty_is_all_text() {
        let view = RttyText {
            text: "RYRYRY".to_owned(),
        }
        .present(EventKind::Rtty);
        assert!(view.fields.is_empty());
        assert_eq!(view.text.as_deref(), Some("RYRYRY"));
    }

    #[test]
    fn morse_and_psk_add_speed_and_mode() {
        let morse = MorseText {
            text: "CQ".to_owned(),
            wpm: 18.4,
        }
        .present(EventKind::Morse);
        assert_eq!(
            morse.field("Speed"),
            number(Some(18.4_f32), Unit::Wpm, 0).as_ref()
        );
        let psk = PskText {
            baud: PskBaud::Psk63,
            text: "CQ".to_owned(),
        }
        .present(EventKind::Psk);
        assert_eq!(psk.field("Mode"), text_of("PSK63").as_ref());
        assert_eq!(psk.text.as_deref(), Some("CQ"));
    }

    #[test]
    fn a_skimmer_spot_shows_offset_speed_and_snr() {
        let view = CwSkimmerSpot {
            offset_hz: -120.0,
            text: "CQ DL1ABC".to_owned(),
            wpm: 24.0,
            snr_db: 11.5,
        }
        .present(EventKind::CwSkimmer);
        assert_eq!(view.labels(), ["Offset", "Speed", "SNR", "Text"]);
        assert_eq!(
            view.field("Offset"),
            number(Some(-120.0_f32), Unit::Hz, 0).as_ref()
        );
        assert_eq!(view.text, None);
    }
}

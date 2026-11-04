use sdrmm_wire::{Capability, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;
use sdrmm_wire::named;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::text_of;
use sdrmm_wire::present::text_pane;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PskBaud {
    #[default]
    Psk31,
    Psk63,
    Psk125,
    Psk250,
}

impl PskBaud {
    pub const ALL: [Self; 4] = [Self::Psk31, Self::Psk63, Self::Psk125, Self::Psk250];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Psk31 => "PSK31",
            Self::Psk63 => "PSK63",
            Self::Psk125 => "PSK125",
            Self::Psk250 => "PSK250",
        }
    }

    #[must_use]
    pub fn rate(self) -> f64 {
        match self {
            Self::Psk31 => 31.25,
            Self::Psk63 => 62.5,
            Self::Psk125 => 125.0,
            Self::Psk250 => 250.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PskParams {
    #[serde(default)]
    pub baud: PskBaud,
    #[serde(default)]
    pub invert: bool,
}

named!(PskBaud);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PskText {
    pub baud: PskBaud,
    pub text: String,
}

impl EventPayload for PskText {
    fn present(&self, _kind: &EventKind) -> Presentation {
        text_pane(
            &self.text,
            fields(&[("mode", "Mode", text_of(self.baud.label()))]),
        )
    }
}

pub(crate) fn psk_kind() -> KindSpec {
    KindSpec::of::<PskText>("PSK", "PSK", Hue::Muted)
        .caps(&[Capability::Log, Capability::Text])
        .tentative()
        .build()
}

impl EventContract for PskText {
    const KIND: EventKind = EventKind::named("psk");
}

impl sdrmm_wire::DecoderSettings for PskParams {
    const TYPE_ID: &'static str = "psk";
}

#[cfg(test)]
mod model_tests {
    use sdrmm_wire::present::{FieldValue, Unit, number};

    use super::*;
    use crate::morse::MorseText;

    fn field<'a>(view: &'a Presentation, label: &str) -> Option<&'a FieldValue> {
        view.fields
            .iter()
            .find(|field| field.label == label)
            .map(|field| &field.value)
    }

    #[test]
    fn morse_and_psk_add_speed_and_mode() {
        let morse = MorseText {
            text: "CQ".to_owned(),
            wpm: 18.4,
        }
        .present(&MorseText::KIND);
        assert_eq!(
            field(&morse, "Speed"),
            number(Some(18.4_f32), Unit::Wpm, 0).as_ref()
        );
        let psk = PskText {
            baud: PskBaud::Psk63,
            text: "CQ".to_owned(),
        }
        .present(&PskText::KIND);
        assert_eq!(field(&psk, "Mode"), text_of("PSK63").as_ref());
        assert_eq!(psk.text.as_deref(), Some("CQ"));
    }
}

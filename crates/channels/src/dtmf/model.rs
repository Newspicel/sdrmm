use sdrmm_wire::present::{EventPayload, Presentation, Unit, fields, number, text};
use sdrmm_wire::{Capability, EventContract, EventKind, Hue, KindSpec, named};
use serde::{Deserialize, Serialize};

/// The press and break lengths a channel holds keys to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DtmfPace {
    #[default]
    Q24,
    Fast,
}

impl DtmfPace {
    pub const ALL: [Self; 2] = [Self::Q24, Self::Fast];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Q24 => "Q.24",
            Self::Fast => "Fast",
        }
    }

    #[must_use]
    pub const fn timing(self) -> sdrmm_dsp::DtmfTiming {
        match self {
            Self::Q24 => sdrmm_dsp::DtmfTiming::Q24,
            Self::Fast => sdrmm_dsp::DtmfTiming::FAST,
        }
    }
}

named!(DtmfPace);

/// The silences a sequence may end on.
pub const DTMF_GAPS_MS: [u32; 5] = [250, 500, 1_000, 1_500, 3_000];

const fn default_dtmf_gap_ms() -> u32 {
    1_500
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DtmfParams {
    #[serde(default)]
    pub pace: DtmfPace,
    #[serde(default = "default_dtmf_gap_ms")]
    pub gap_ms: u32,
}

impl Default for DtmfParams {
    fn default() -> Self {
        Self {
            pace: DtmfPace::default(),
            gap_ms: default_dtmf_gap_ms(),
        }
    }
}

impl sdrmm_wire::DecoderSettings for DtmfParams {
    const TYPE_ID: &'static str = "dtmf";
}

/// Keys dialed with no silence of the channel's gap between them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DtmfSequence {
    pub digits: String,
    pub tone_ms: u32,
    pub gap_ms: Option<u32>,
}

impl EventPayload for DtmfSequence {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let seconds = |ms: u32| f64::from(ms) / 1_000.0;
        Presentation {
            fields: fields(&[
                ("digits", "Digits", text(Some(&self.digits))),
                (
                    "tone",
                    "Tone",
                    number(Some(seconds(self.tone_ms)), Unit::Seconds, 3),
                ),
                (
                    "gap",
                    "Gap",
                    number(self.gap_ms.map(seconds), Unit::Seconds, 3),
                ),
            ]),
            ..Presentation::of(self.digits.clone())
        }
    }
}

impl EventContract for DtmfSequence {
    const KIND: EventKind = EventKind::named("dtmf");
}

pub(crate) fn dtmf_kind() -> KindSpec {
    KindSpec::of::<DtmfSequence>("DTMF", "DTMF", Hue::Muted)
        .caps(&[Capability::Log])
        .build()
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

    #[test]
    fn a_sequence_shows_its_digits_tone_and_gap() {
        let view = serde_json::from_value::<DtmfSequence>(
            serde_json::json!({ "digits": "461###333", "tone_ms": 40, "gap_ms": 20 }),
        )
        .expect("payload parses")
        .present(&DtmfSequence::KIND);
        assert_eq!(view.summary, "461###333");
        assert_eq!(field(&view, "Digits"), text(Some("461###333")).as_ref());
        assert_eq!(
            field(&view, "Gap"),
            number(Some(0.02), Unit::Seconds, 3).as_ref()
        );
    }

    #[test]
    fn a_single_key_has_no_gap_field() {
        let view = DtmfSequence {
            digits: "5".to_owned(),
            tone_ms: 100,
            gap_ms: None,
        }
        .present(&DtmfSequence::KIND);
        assert_eq!(
            view.fields
                .iter()
                .map(|field| field.label.as_str())
                .collect::<Vec<_>>(),
            ["Digits", "Tone"]
        );
    }

    #[test]
    fn settings_default_to_q24_and_a_one_and_a_half_second_gap() {
        let params: DtmfParams = serde_json::from_str("{}").expect("parses");
        assert_eq!(params, DtmfParams::default());
        assert_eq!(params.pace, DtmfPace::Q24);
        assert_eq!(params.gap_ms, 1_500);
    }
}

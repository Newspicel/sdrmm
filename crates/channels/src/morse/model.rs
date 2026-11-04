use sdrmm_wire::{Capability, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::number;
use sdrmm_wire::present::text_pane;

fn default_morse_bandwidth_hz() -> f64 {
    400.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MorseParams {
    #[serde(default = "default_morse_bandwidth_hz")]
    pub bandwidth_hz: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wpm: Option<f32>,
}

impl Default for MorseParams {
    fn default() -> Self {
        Self {
            bandwidth_hz: default_morse_bandwidth_hz(),
            wpm: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MorseText {
    pub text: String,
    pub wpm: f32,
}

impl EventPayload for MorseText {
    fn present(&self, _kind: &EventKind) -> Presentation {
        text_pane(
            &self.text,
            fields(&[("speed", "Speed", number(Some(self.wpm), Unit::Wpm, 0))]),
        )
    }
}

pub(crate) fn morse_kind() -> KindSpec {
    KindSpec::of::<MorseText>("Morse", "Morse", Hue::Muted)
        .caps(&[Capability::Log, Capability::Text])
        .tentative()
        .build()
}

impl EventContract for MorseText {
    const KIND: EventKind = EventKind::named("morse");
}

impl sdrmm_wire::DecoderSettings for MorseParams {
    const TYPE_ID: &'static str = "morse";
}

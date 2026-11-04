use crate::default_true;
use sdrmm_wire::{Capability, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;
use sdrmm_wire::labelled;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::text_pane;

fn default_rtty_baud() -> f64 {
    45.45
}

fn default_rtty_shift_hz() -> f64 {
    170.0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RttyStopBits {
    One,
    #[default]
    OneAndHalf,
    Two,
}

impl RttyStopBits {
    #[must_use]
    pub fn periods(self) -> f64 {
        match self {
            Self::One => 1.0,
            Self::OneAndHalf => 1.5,
            Self::Two => 2.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RttyParams {
    #[serde(default = "default_rtty_baud")]
    pub baud: f64,
    #[serde(default = "default_rtty_shift_hz")]
    pub shift_hz: f64,
    #[serde(default)]
    pub stop_bits: RttyStopBits,
    #[serde(default)]
    pub invert: bool,
    #[serde(default = "default_true")]
    pub unshift_on_space: bool,
}

impl Default for RttyParams {
    fn default() -> Self {
        Self {
            baud: default_rtty_baud(),
            shift_hz: default_rtty_shift_hz(),
            stop_bits: RttyStopBits::default(),
            invert: false,
            unshift_on_space: true,
        }
    }
}

pub const RTTY_BAUDS: [f64; 3] = [45.45, 50.0, 75.0];

pub const RTTY_SHIFTS_HZ: [f64; 3] = [170.0, 450.0, 850.0];

labelled! {
    RttyStopBits { One => "1", OneAndHalf => "1.5", Two => "2" }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RttyText {
    pub text: String,
}

impl EventPayload for RttyText {
    fn present(&self, _kind: &EventKind) -> Presentation {
        text_pane(&self.text, Vec::new())
    }
}

pub(crate) fn rtty_kind() -> KindSpec {
    KindSpec::of::<RttyText>("RTTY", "RTTY", Hue::Muted)
        .caps(&[Capability::Log, Capability::Text])
        .tentative()
        .build()
}

impl EventContract for RttyText {
    const KIND: EventKind = EventKind::named("rtty");
}

impl sdrmm_wire::DecoderSettings for RttyParams {
    const TYPE_ID: &'static str = "rtty";
}

#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn rtty_is_all_text() {
        let view = RttyText {
            text: "RYRYRY".to_owned(),
        }
        .present(&RttyText::KIND);
        assert!(view.fields.is_empty());
        assert_eq!(view.text.as_deref(), Some("RYRYRY"));
    }
}

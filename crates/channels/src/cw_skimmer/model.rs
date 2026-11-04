use sdrmm_wire::{Capability, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::number;
use sdrmm_wire::present::text;

fn default_cw_skimmer_bandwidth_hz() -> f64 {
    24_000.0
}

fn default_cw_skimmer_threshold_db() -> f32 {
    10.0
}

fn default_cw_skimmer_max_signals() -> u16 {
    32
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CwSkimmerParams {
    #[serde(default = "default_cw_skimmer_bandwidth_hz")]
    pub bandwidth_hz: f64,
    #[serde(default = "default_cw_skimmer_threshold_db")]
    pub threshold_db: f32,
    #[serde(default = "default_cw_skimmer_max_signals")]
    pub max_signals: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wpm: Option<f32>,
}

impl Default for CwSkimmerParams {
    fn default() -> Self {
        Self {
            bandwidth_hz: default_cw_skimmer_bandwidth_hz(),
            threshold_db: default_cw_skimmer_threshold_db(),
            max_signals: default_cw_skimmer_max_signals(),
            wpm: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CwSkimmerSpot {
    pub offset_hz: f32,
    pub text: String,
    pub wpm: f32,
    pub snr_db: f32,
}

impl EventPayload for CwSkimmerSpot {
    fn present(&self, _kind: &EventKind) -> Presentation {
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

pub(crate) fn cw_skimmer_kind() -> KindSpec {
    KindSpec::of::<CwSkimmerSpot>("CW skimmer", "CW spots", Hue::Muted)
        .caps(&[Capability::Log])
        .build()
}

impl EventContract for CwSkimmerSpot {
    const KIND: EventKind = EventKind::named("cw_skimmer");
}

impl sdrmm_wire::DecoderSettings for CwSkimmerParams {
    const TYPE_ID: &'static str = "cw_skimmer";
}

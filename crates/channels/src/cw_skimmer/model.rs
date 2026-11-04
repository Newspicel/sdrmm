use sdrmm_wire::{Capability, EventContract, Hue, KindSpec, KindView};
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

/// Spots this close together come from one signal, whose tracked offset wanders a little.
const SIGNAL_GROUP_HZ: f32 = 50.0;

impl CwSkimmerSpot {
    /// The offset the spot's signal is known by, so its spots share one row.
    #[must_use]
    pub fn signal_hz(&self) -> f32 {
        (self.offset_hz / SIGNAL_GROUP_HZ).round() * SIGNAL_GROUP_HZ + 0.0
    }
}

impl EventPayload for CwSkimmerSpot {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            fields: fields(&[
                (
                    "signal",
                    "Signal",
                    number(Some(self.signal_hz()), Unit::Hz, 0),
                ),
                (
                    "offset",
                    "Offset",
                    number(Some(self.offset_hz), Unit::Hz, 0),
                ),
                ("speed", "Speed", number(Some(self.wpm), Unit::Wpm, 0)),
                ("snr", "SNR", number(Some(self.snr_db), Unit::Db, 1)),
                ("text", "Text", text(Some(&self.text))),
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
        .view(
            KindView::table(&["signal", "speed", "snr", "text"])
                .sorted("signal", false)
                .keyed("signal"),
        )
        .build()
}

impl EventContract for CwSkimmerSpot {
    const KIND: EventKind = EventKind::named("cw_skimmer");
}

impl sdrmm_wire::DecoderSettings for CwSkimmerParams {
    const TYPE_ID: &'static str = "cw_skimmer";
}

#[cfg(test)]
mod model_tests {
    use super::*;

    fn spot(offset_hz: f32) -> CwSkimmerSpot {
        CwSkimmerSpot {
            offset_hz,
            text: "CQ".to_owned(),
            wpm: 24.0,
            snr_db: 12.0,
        }
    }

    #[test]
    fn a_wandering_signal_keeps_one_row() {
        assert_eq!(spot(1_237.0).signal_hz(), 1_250.0);
        assert_eq!(spot(1_262.0).signal_hz(), 1_250.0);
        assert_eq!(spot(-1_237.0).signal_hz(), -1_250.0);
        assert_eq!(spot(-10.0).signal_hz().to_bits(), 0.0_f32.to_bits());
        crate::testutil::assert_views_hold(
            &cw_skimmer_kind(),
            &[sdrmm_wire::DecoderEvent::of(&spot(1_237.0))],
        );
    }
}

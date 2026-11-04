use sdrmm_wire::{Capability, DecoderSettings, EventContract, Hue, KindSpec, KindView};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::labelled;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::count;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::number;
use sdrmm_wire::present::text_of;

labelled! {
    PerBaud { Baud1200 => "1200", Baud2400 => "2400", Baud4800 => "4800", Baud9600 => "9600" }
}

pub const MIN_PER_PAYLOAD_BYTES: u16 = 1;

pub const MAX_PER_PAYLOAD_BYTES: u16 = 255;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerBaud {
    Baud1200,
    Baud2400,
    Baud4800,
    #[default]
    Baud9600,
}

impl PerBaud {
    pub const ALL: [Self; 4] = [
        Self::Baud1200,
        Self::Baud2400,
        Self::Baud4800,
        Self::Baud9600,
    ];

    #[must_use]
    pub fn rate(self) -> f64 {
        match self {
            Self::Baud1200 => 1_200.0,
            Self::Baud2400 => 2_400.0,
            Self::Baud4800 => 4_800.0,
            Self::Baud9600 => 9_600.0,
        }
    }
}

fn default_per_payload_bytes() -> u16 {
    32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerParams {
    #[serde(default)]
    pub baud: PerBaud,
    #[serde(default = "default_per_payload_bytes")]
    pub payload_bytes: u16,
}

impl Default for PerParams {
    fn default() -> Self {
        Self {
            baud: PerBaud::default(),
            payload_bytes: default_per_payload_bytes(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PerReport {
    pub received: u64,
    pub errored: u64,
    pub missed: u64,
    pub expected: u64,
    pub per: f32,
    pub bits: u64,
    pub bit_errors: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ber: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sequence: Option<u32>,
    #[serde(default)]
    pub restarts: u32,
}

impl PerReport {
    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = vec![
            format!("PER {:.2}%", self.per * 100.0),
            format!("{}/{}", self.received, self.expected),
        ];
        match self.ber {
            Some(0.0) => parts.push("BER 0".to_owned()),
            Some(ber) => parts.push(format!("BER {ber:.1e}")),
            None => {}
        }
        parts.join(" · ")
    }
}

impl EventPayload for PerReport {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            fields: fields(&[
                (
                    "per",
                    "PER",
                    number(Some(self.per * 100.0), Unit::Percent, 2),
                ),
                (
                    "received",
                    "Received",
                    text_of(format!("{} of {}", self.received, self.expected)),
                ),
                ("missed", "Missed", count(Some(self.missed as f64))),
                ("crc_failed", "CRC failed", count(Some(self.errored as f64))),
                (
                    "ber",
                    "BER",
                    self.ber.and_then(|ber| text_of(format!("{ber:.2e}"))),
                ),
                (
                    "bit_errors",
                    "Bit errors",
                    text_of(format!("{} of {}", self.bit_errors, self.bits)),
                ),
                ("last_frame", "Last frame", count(self.last_sequence)),
                (
                    "restarts",
                    "Restarts",
                    count(Some(self.restarts).filter(|restarts| *restarts > 0)),
                ),
            ]),
            ..Presentation::of(self.summary())
        }
    }
}

/// Long enough to watch a link change as an antenna or level is adjusted.
const PER_TREND_S: u32 = 600;

pub(crate) fn per_kind() -> KindSpec {
    KindSpec::of::<PerReport>("PER", "PER", Hue::Muted)
        .caps(&[Capability::Log])
        .view(KindView::meter("per", 0.0, 100.0))
        .view(KindView::trend(&["per"], PER_TREND_S))
        .build()
}

impl EventContract for PerReport {
    const KIND: EventKind = EventKind::named("per");
}

impl DecoderSettings for PerParams {
    const TYPE_ID: &'static str = "per";
}

#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn a_per_report_counts_frames_and_bits() {
        let view = PerReport {
            received: 98,
            errored: 1,
            missed: 1,
            expected: 100,
            per: 0.02,
            bits: 25_088,
            bit_errors: 3,
            ber: Some(0.000_12),
            last_sequence: Some(99),
            restarts: 0,
        }
        .present(&EventKind::named("per"));
        assert_eq!(view.field("Received"), text_of("98 of 100").as_ref());
        assert_eq!(view.field("BER"), text_of("1.20e-4").as_ref());
        assert_eq!(view.field("Restarts"), None);
    }

    #[test]
    fn the_meter_and_trend_read_the_packet_error_rate() {
        let report = PerReport {
            received: 98,
            expected: 100,
            per: 0.02,
            ..PerReport::default()
        };
        crate::testutil::assert_views_hold(&per_kind(), &[sdrmm_wire::DecoderEvent::of(&report)]);
    }
}

use sdrmm_wire::{Capability, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::named;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::count;
use sdrmm_wire::present::picture_fields;
use sdrmm_wire::present::picture_size;
use sdrmm_wire::present::text;
use sdrmm_wire::present::text_of;

named!(LrptMode);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LrptMode {
    Qpsk72,
    #[default]
    Oqpsk72,
    Oqpsk80,
}

impl LrptMode {
    pub const ALL: [Self; 3] = [Self::Qpsk72, Self::Oqpsk72, Self::Oqpsk80];

    #[must_use]
    pub const fn symbol_rate(self) -> f64 {
        match self {
            Self::Qpsk72 | Self::Oqpsk72 => 72_000.0,
            Self::Oqpsk80 => 80_000.0,
        }
    }

    #[must_use]
    pub const fn offset(self) -> bool {
        matches!(self, Self::Oqpsk72 | Self::Oqpsk80)
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Qpsk72 => "QPSK 72k",
            Self::Oqpsk72 => "OQPSK 72k",
            Self::Oqpsk80 => "OQPSK 80k",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LrptParams {
    #[serde(default)]
    pub mode: LrptMode,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LrptImage {
    pub seq: u32,
    pub mode: LrptMode,
    pub width: u16,
    pub lines: u16,
    pub complete: bool,
    pub duration_ms: u32,
    pub apids: Vec<u16>,
    pub frames: u32,
    pub frames_corrected: u32,
    pub frames_failed: u32,
    pub packets_lost: u32,
}

fn lrpt_summary(p: &LrptImage) -> String {
    let apids = p
        .apids
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join("/");
    let mut parts = vec![p.mode.label().to_owned()];
    if !apids.is_empty() {
        parts.push(format!("APID {apids}"));
    }
    parts.push(format!("{} lines", p.lines));
    if p.frames_failed > 0 || p.packets_lost > 0 {
        parts.push(format!(
            "{} frames lost · {} packets lost",
            p.frames_failed, p.packets_lost
        ));
    }
    parts.join(" · ")
}

impl EventPayload for LrptImage {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let apids = self
            .apids
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        Presentation {
            fields: picture_fields(
                &[
                    ("Mode", text_of(self.mode.label())),
                    ("Size", picture_size(self.width, self.lines)),
                    ("APIDs", text(Some(&apids))),
                    ("Frames", count(Some(self.frames))),
                    ("Frames repaired", count(Some(self.frames_corrected))),
                    ("Frames failed", count(Some(self.frames_failed))),
                    ("Packets lost", count(Some(self.packets_lost))),
                ],
                self.lines,
                self.complete,
                self.duration_ms,
            ),
            ..Presentation::of(lrpt_summary(self))
        }
    }
}

pub(crate) fn lrpt_kind() -> KindSpec {
    KindSpec::of::<LrptImage>("Meteor LRPT", "Pictures", Hue::Orange)
        .caps(&[Capability::Log, Capability::Picture])
        .build()
}

impl EventContract for LrptImage {
    const KIND: EventKind = EventKind::named("lrpt");
}

impl sdrmm_wire::DecoderSettings for LrptParams {
    const TYPE_ID: &'static str = "lrpt";
}

#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn lrpt_shows_link_quality() {
        let view = LrptImage {
            seq: 1,
            mode: LrptMode::Oqpsk80,
            width: 1568,
            lines: 400,
            complete: true,
            duration_ms: 1000,
            apids: vec![64, 65],
            frames: 500,
            frames_corrected: 12,
            frames_failed: 1,
            packets_lost: 4,
        }
        .present(&EventKind::named("lrpt"));
        assert_eq!(view.field("Mode"), text_of("OQPSK 80k").as_ref());
        assert_eq!(view.field("APIDs"), text_of("64, 65").as_ref());
        assert_eq!(view.field("Frames repaired"), count(Some(12)).as_ref());
        assert_eq!(view.field("Packets lost"), count(Some(4)).as_ref());
    }
}

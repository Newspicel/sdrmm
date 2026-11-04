use crate::default_true;
use sdrmm_wire::{Capability, EventContract, Hue, KindSpec, KindView};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::labelled;
use sdrmm_wire::named;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::joined;
use sdrmm_wire::present::picture_fields;
use sdrmm_wire::present::picture_size;
use sdrmm_wire::present::text_of;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SstvMode {
    Robot36,
    Robot72,
    MartinM1,
    MartinM2,
    ScottieS1,
    ScottieS2,
    ScottieDx,
    Pd50,
    Pd90,
    Pd120,
    Pd180,
    Sc2180,
}

impl SstvMode {
    pub const ALL: [Self; 12] = [
        Self::Robot36,
        Self::Robot72,
        Self::MartinM1,
        Self::MartinM2,
        Self::ScottieS1,
        Self::ScottieS2,
        Self::ScottieDx,
        Self::Pd50,
        Self::Pd90,
        Self::Pd120,
        Self::Pd180,
        Self::Sc2180,
    ];

    #[must_use]
    pub fn vis(self) -> u8 {
        match self {
            Self::Robot36 => 8,
            Self::Robot72 => 12,
            Self::MartinM2 => 40,
            Self::MartinM1 => 44,
            Self::Sc2180 => 55,
            Self::ScottieS2 => 56,
            Self::ScottieS1 => 60,
            Self::ScottieDx => 76,
            Self::Pd50 => 93,
            Self::Pd120 => 95,
            Self::Pd180 => 96,
            Self::Pd90 => 99,
        }
    }

    #[must_use]
    pub fn from_vis(vis: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.vis() == vis)
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Robot36 => "Robot 36",
            Self::Robot72 => "Robot 72",
            Self::MartinM1 => "Martin M1",
            Self::MartinM2 => "Martin M2",
            Self::ScottieS1 => "Scottie S1",
            Self::ScottieS2 => "Scottie S2",
            Self::ScottieDx => "Scottie DX",
            Self::Pd50 => "PD50",
            Self::Pd90 => "PD90",
            Self::Pd120 => "PD120",
            Self::Pd180 => "PD180",
            Self::Sc2180 => "Wraase SC2-180",
        }
    }

    #[must_use]
    pub fn size(self) -> (u16, u16) {
        match self {
            Self::Robot36 | Self::Robot72 => (320, 240),
            Self::MartinM1
            | Self::MartinM2
            | Self::ScottieS1
            | Self::ScottieS2
            | Self::ScottieDx
            | Self::Pd50
            | Self::Pd90
            | Self::Sc2180 => (320, 256),
            Self::Pd120 | Self::Pd180 => (640, 496),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SstvModulation {
    #[default]
    Usb,
    Lsb,
    Fm,
    Am,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SstvParams {
    #[serde(default)]
    pub modulation: SstvModulation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<SstvMode>,
    #[serde(default = "default_true")]
    pub slant_correction: bool,
    #[serde(default = "default_true")]
    pub keep_partial: bool,
}

impl Default for SstvParams {
    fn default() -> Self {
        Self {
            modulation: SstvModulation::default(),
            mode: None,
            slant_correction: true,
            keep_partial: true,
        }
    }
}

named!(SstvMode);

labelled! {
    SstvModulation { Usb => "USB", Lsb => "LSB", Fm => "FM", Am => "AM" }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SstvPicture {
    pub seq: u32,
    pub mode: SstvMode,
    pub width: u16,
    pub height: u16,
    pub lines: u16,
    pub complete: bool,
    pub duration_ms: u32,
}

impl EventPayload for SstvPicture {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let outcome = if self.complete {
            format!("complete in {} s", self.duration_ms / 1_000)
        } else {
            format!("{} of {} lines", self.lines, self.height)
        };
        Presentation {
            fields: picture_fields(
                &[
                    ("mode", "Mode", text_of(self.mode.label())),
                    ("size", "Size", picture_size(self.width, self.height)),
                ],
                self.lines,
                self.complete,
                self.duration_ms,
            ),
            ..Presentation::of(joined([
                self.mode.label().to_owned(),
                format!("{}×{}", self.width, self.height),
                outcome,
            ]))
        }
    }
}

pub(crate) fn sstv_kind() -> KindSpec {
    KindSpec::of::<SstvPicture>("SSTV", "Pictures", Hue::Orange)
        .caps(&[Capability::Log, Capability::Picture])
        .view(KindView::Picture)
        .build()
}

impl EventContract for SstvPicture {
    const KIND: EventKind = EventKind::named("sstv");
}

impl sdrmm_wire::DecoderSettings for SstvParams {
    const TYPE_ID: &'static str = "sstv";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::DecoderEvent;
    use sdrmm_wire::present::Unit;
    use sdrmm_wire::present::flag;
    use sdrmm_wire::present::number;

    #[test]
    fn an_sstv_picture_summarises_its_mode_size_and_outcome() {
        let complete = DecoderEvent::of(&SstvPicture {
            seq: 1,
            mode: SstvMode::MartinM1,
            width: 320,
            height: 256,
            lines: 256,
            complete: true,
            duration_ms: 114_300,
        });
        assert_eq!(complete.kind(), EventKind::named("sstv"));
        assert_eq!(
            complete.present().summary,
            "Martin M1 · 320×256 · complete in 114 s"
        );
        assert_eq!(complete.present().entity, None);

        let cut_short = DecoderEvent::of(&SstvPicture {
            seq: 2,
            mode: SstvMode::Robot36,
            width: 320,
            height: 240,
            lines: 96,
            complete: false,
            duration_ms: 14_500,
        });
        assert_eq!(
            cut_short.present().summary,
            "Robot 36 · 320×240 · 96 of 240 lines"
        );
    }

    #[test]
    fn sstv_shows_mode_size_and_progress() {
        let view = SstvPicture {
            seq: 1,
            mode: SstvMode::MartinM1,
            width: 320,
            height: 256,
            lines: 200,
            complete: false,
            duration_ms: 90_500,
        }
        .present(&EventKind::named("sstv"));
        assert_eq!(view.labels(), ["Mode", "Size", "Lines", "Complete", "Took"]);
        assert_eq!(view.field("Mode"), text_of("Martin M1").as_ref());
        assert_eq!(view.field("Size"), text_of("320 × 256").as_ref());
        assert_eq!(view.field("Complete"), flag(Some(false)).as_ref());
        assert_eq!(
            view.field("Took"),
            number(Some(90.5), Unit::Seconds, 1).as_ref()
        );
        assert_eq!(view.entity, None);
    }

    #[test]
    fn pictures_name_no_station() {
        fn entity<T: EventPayload>(kind: &'static str, json: &str) -> Option<String> {
            serde_json::from_str::<T>(json)
                .expect("picture fixture")
                .present(&EventKind::named(kind))
                .entity
        }
        assert_eq!(
            entity::<SstvPicture>(
                "sstv",
                r#"{"seq":1,"mode":"martin_m1","width":320,"height":256,"lines":256,"complete":true,"duration_ms":114300}"#
            ),
            None
        );
        assert_eq!(
            entity::<crate::apt::AptImage>(
                "apt",
                r#"{"seq":1,"lines":1200,"complete":true,"duration_ms":600000}"#
            ),
            None
        );
        assert_eq!(
            entity::<crate::lrpt::LrptImage>(
                "lrpt",
                r#"{"seq":1,"mode":"qpsk72","width":1568,"lines":800,"complete":true,"duration_ms":600000,"apids":[64,65,66],"frames":100,"frames_corrected":2,"frames_failed":0,"packets_lost":0}"#
            ),
            None
        );
        assert_eq!(
            entity::<crate::wefax::WefaxPicture>(
                "wefax",
                r#"{"seq":1,"ioc":"ioc576","lpm":"lpm120","width":1809,"lines":1200,"complete":true,"duration_ms":600000}"#
            ),
            None
        );
    }
}

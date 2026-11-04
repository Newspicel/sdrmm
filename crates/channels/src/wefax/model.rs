use crate::default_true;
use sdrmm_wire::{Capability, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::labelled;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::count;
use sdrmm_wire::present::picture_fields;
use sdrmm_wire::present::picture_size;

labelled! {
    WefaxIoc { Ioc576 => "576", Ioc288 => "288" }
}

labelled! {
    WefaxLpm { Lpm60 => "60", Lpm90 => "90", Lpm120 => "120", Lpm240 => "240" }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WefaxIoc {
    #[default]
    Ioc576,
    Ioc288,
}

impl WefaxIoc {
    pub const ALL: [Self; 2] = [Self::Ioc576, Self::Ioc288];

    #[must_use]
    pub const fn value(self) -> u16 {
        match self {
            Self::Ioc576 => 576,
            Self::Ioc288 => 288,
        }
    }

    #[must_use]
    pub fn width(self) -> u16 {
        (f64::from(self.value()) * std::f64::consts::PI).round() as u16
    }

    #[must_use]
    pub const fn start_tone_hz(self) -> f64 {
        match self {
            Self::Ioc576 => 300.0,
            Self::Ioc288 => 675.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WefaxLpm {
    Lpm60,
    Lpm90,
    #[default]
    Lpm120,
    Lpm240,
}

impl WefaxLpm {
    pub const ALL: [Self; 4] = [Self::Lpm60, Self::Lpm90, Self::Lpm120, Self::Lpm240];

    #[must_use]
    pub const fn value(self) -> u16 {
        match self {
            Self::Lpm60 => 60,
            Self::Lpm90 => 90,
            Self::Lpm120 => 120,
            Self::Lpm240 => 240,
        }
    }

    #[must_use]
    pub fn line_ms(self) -> f64 {
        60_000.0 / f64::from(self.value())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WefaxParams {
    #[serde(default)]
    pub ioc: WefaxIoc,
    #[serde(default)]
    pub lpm: WefaxLpm,
    #[serde(default = "default_true")]
    pub keep_partial: bool,
}

impl Default for WefaxParams {
    fn default() -> Self {
        Self {
            ioc: WefaxIoc::default(),
            lpm: WefaxLpm::default(),
            keep_partial: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WefaxPicture {
    pub seq: u32,
    pub ioc: WefaxIoc,
    pub lpm: WefaxLpm,
    pub width: u16,
    pub lines: u16,
    pub complete: bool,
    pub duration_ms: u32,
}

fn wefax_summary(p: &WefaxPicture) -> String {
    let mut parts = vec![format!("IOC {} · {} LPM", p.ioc.value(), p.lpm.value())];
    parts.push(if p.complete {
        format!("{} lines in {} s", p.lines, p.duration_ms / 1_000)
    } else {
        format!("{} lines, cut short", p.lines)
    });
    parts.join(" · ")
}

impl EventPayload for WefaxPicture {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            fields: picture_fields(
                &[
                    ("IOC", count(Some(self.ioc.value()))),
                    ("LPM", count(Some(self.lpm.value()))),
                    ("Size", picture_size(self.width, self.lines)),
                ],
                self.lines,
                self.complete,
                self.duration_ms,
            ),
            ..Presentation::of(wefax_summary(self))
        }
    }
}

pub(crate) fn wefax_kind() -> KindSpec {
    KindSpec::of::<WefaxPicture>("WEFAX", "Pictures", Hue::Orange)
        .caps(&[Capability::Log, Capability::Picture])
        .build()
}

impl EventContract for WefaxPicture {
    const KIND: EventKind = EventKind::named("wefax");
}

impl sdrmm_wire::DecoderSettings for WefaxParams {
    const TYPE_ID: &'static str = "wefax";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::present::text_of;

    #[test]
    fn wefax_geometry_follows_the_index_of_cooperation() {
        assert_eq!(WefaxIoc::Ioc576.width(), 1810);
        assert_eq!(WefaxIoc::Ioc288.width(), 905);
        assert_eq!(WefaxLpm::Lpm120.line_ms(), 500.0);
        assert_eq!(WefaxLpm::Lpm60.line_ms(), 1_000.0);
    }

    #[test]
    fn wefax_shows_its_timing() {
        let view = WefaxPicture {
            seq: 1,
            ioc: WefaxIoc::Ioc288,
            lpm: WefaxLpm::Lpm60,
            width: 904,
            lines: 300,
            complete: true,
            duration_ms: 300_000,
        }
        .present(&EventKind::named("wefax"));
        assert_eq!(view.field("IOC"), count(Some(288)).as_ref());
        assert_eq!(view.field("LPM"), count(Some(60)).as_ref());
        assert_eq!(view.field("Size"), text_of("904 × 300").as_ref());
    }
}

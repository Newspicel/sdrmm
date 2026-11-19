use sdrmm_wire::{Capability, EventContract, Hue, KindSpec, KindView};
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

named!(HrptSatellite, HrptPicture);

/// Whose downlink: they share the carrier and bit rate but not the framing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HrptSatellite {
    /// Meteor-M N2-3 and N2-4, MSU-MR at 1572 pixels.
    #[default]
    Meteor,
    /// NOAA-15, 18 and 19, AVHRR at 2048 pixels.
    Noaa,
    /// `MetOp`-B and C AHRPT, AVHRR at 2048 pixels.
    Metop,
}

impl HrptSatellite {
    pub const ALL: [Self; 3] = [Self::Meteor, Self::Noaa, Self::Metop];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Meteor => "Meteor-M",
            Self::Noaa => "NOAA",
            Self::Metop => "MetOp",
        }
    }
}

/// Which imager channels make the picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HrptPicture {
    /// Visible in daylight, infrared at night.
    #[default]
    Auto,
    /// Channels 2, 2 and 1 as red, green and blue.
    Visible,
    /// Near 11 µm, cold clouds white: MSU-MR channel 5, AVHRR channel 4.
    Infrared,
}

impl HrptPicture {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Visible, Self::Infrared];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Visible => "Visible 221",
            Self::Infrared => "Infrared",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HrptParams {
    #[serde(default)]
    pub satellite: HrptSatellite,
    #[serde(default)]
    pub picture: HrptPicture,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HrptImage {
    pub seq: u32,
    pub satellite: HrptSatellite,
    /// What the picture shows, never `Auto`.
    pub picture: HrptPicture,
    pub width: u16,
    pub lines: u16,
    pub complete: bool,
    pub duration_ms: u32,
    /// Onboard time of the first line, UTC.
    pub first_line: Option<String>,
    pub frames: u32,
    pub frames_damaged: u32,
}

fn hrpt_summary(p: &HrptImage) -> String {
    let mut parts = vec![
        p.satellite.label().to_owned(),
        p.picture.label().to_owned(),
        format!("{} lines", p.lines),
    ];
    if p.frames_damaged > 0 {
        parts.push(format!("{} frames damaged", p.frames_damaged));
    }
    parts.join(" · ")
}

impl EventPayload for HrptImage {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            fields: picture_fields(
                &[
                    ("satellite", "Satellite", text_of(self.satellite.label())),
                    ("picture", "Picture", text_of(self.picture.label())),
                    ("size", "Size", picture_size(self.width, self.lines)),
                    ("first_line", "First line", text(self.first_line.as_deref())),
                    ("frames", "Frames", count(Some(self.frames))),
                    (
                        "frames_damaged",
                        "Frames damaged",
                        count(Some(self.frames_damaged)),
                    ),
                ],
                self.lines,
                self.complete,
                self.duration_ms,
            ),
            ..Presentation::of(hrpt_summary(self))
        }
    }
}

pub(crate) fn hrpt_kind() -> KindSpec {
    KindSpec::of::<HrptImage>("HRPT", "Pictures", Hue::Orange)
        .caps(&[Capability::Log, Capability::Picture])
        .view(KindView::Picture)
        .build()
}

impl EventContract for HrptImage {
    const KIND: EventKind = EventKind::named("hrpt");
}

impl sdrmm_wire::DecoderSettings for HrptParams {
    const TYPE_ID: &'static str = "hrpt";
}

#[cfg(test)]
mod model_tests {
    use super::*;

    fn image() -> HrptImage {
        HrptImage {
            seq: 1,
            satellite: HrptSatellite::Meteor,
            picture: HrptPicture::Infrared,
            width: 1572,
            lines: 40,
            complete: true,
            duration_ms: 6_000,
            first_line: Some("05:22:25 UTC".to_owned()),
            frames: 500,
            frames_damaged: 3,
        }
    }

    #[test]
    fn empty_settings_take_the_defaults() {
        let params = serde_json::from_str::<HrptParams>("{}").unwrap();
        assert_eq!(params.satellite, HrptSatellite::Meteor);
        assert_eq!(params.picture, HrptPicture::Auto);
    }

    #[test]
    fn hrpt_shows_link_quality_and_time() {
        let view = image().present(&EventKind::named("hrpt"));
        assert_eq!(view.field("Picture"), text_of("Infrared").as_ref());
        assert_eq!(view.field("First line"), text_of("05:22:25 UTC").as_ref());
        assert_eq!(view.field("Frames damaged"), count(Some(3)).as_ref());
        assert_eq!(
            view.summary,
            "Meteor-M · Infrared · 40 lines · 3 frames damaged"
        );
        assert_eq!(view.entity, None);
    }
}

use sdrmm_wire::present::{
    EventPayload, Presentation, count, fields, flag, picture_size, text, text_of,
};
use sdrmm_wire::{Capability, EventContract, EventKind, Hue, KindSpec, KindView, named};
use serde::{Deserialize, Serialize};

named!(GoesPicture);

/// Which ABI images are drawn; every file is still logged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoesPicture {
    #[default]
    FullDisk,
    Mesoscale,
    All,
}

impl GoesPicture {
    pub const ALL: [Self; 3] = [Self::FullDisk, Self::Mesoscale, Self::All];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::FullDisk => "Full disk",
            Self::Mesoscale => "Mesoscale",
            Self::All => "All",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoesHritParams {
    #[serde(default)]
    pub picture: GoesPicture,
}

/// One ABI image drawn from its segments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoesHritImage {
    pub seq: u32,
    /// The file name of its first segment, as the annotation header gives it.
    pub name: String,
    pub satellite: Option<String>,
    pub product: String,
    pub band: Option<u8>,
    pub width: u16,
    pub lines: u16,
    pub segments: u16,
    pub segments_expected: u16,
    pub complete: bool,
    pub lines_missing: u32,
}

fn summary(p: &GoesHritImage) -> String {
    let mut parts: Vec<String> = p.satellite.iter().cloned().collect();
    parts.push(p.product.clone());
    if let Some(band) = p.band {
        parts.push(format!("band {band}"));
    }
    parts.push(format!("{}/{} segments", p.segments, p.segments_expected));
    if p.lines_missing > 0 {
        parts.push(format!("{} lines lost", p.lines_missing));
    }
    parts.join(" · ")
}

impl EventPayload for GoesHritImage {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            fields: fields(&[
                ("satellite", "Satellite", text(self.satellite.as_deref())),
                ("product", "Product", text_of(&self.product)),
                ("band", "Band", count(self.band)),
                ("size", "Size", picture_size(self.width, self.lines)),
                ("segments", "Segments", count(Some(self.segments))),
                ("complete", "Complete", flag(Some(self.complete))),
                (
                    "lines_missing",
                    "Lines lost",
                    count(Some(self.lines_missing)),
                ),
                ("name", "File", text_of(&self.name)),
            ]),
            ..Presentation::of(summary(self))
        }
    }
}

pub(crate) fn goes_hrit_kind() -> KindSpec {
    KindSpec::of::<GoesHritImage>("GOES HRIT", "Pictures", Hue::Orange)
        .caps(&[Capability::Log, Capability::Picture])
        .view(KindView::Picture)
        .build()
}

impl EventContract for GoesHritImage {
    const KIND: EventKind = EventKind::named("goes_hrit");
}

impl sdrmm_wire::DecoderSettings for GoesHritParams {
    const TYPE_ID: &'static str = "goes_hrit";
}

#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn empty_settings_draw_the_full_disk() {
        let params: GoesHritParams = serde_json::from_str("{}").expect("defaults");
        assert_eq!(params.picture, GoesPicture::FullDisk);
    }

    #[test]
    fn an_image_names_its_product_and_losses() {
        let view = GoesHritImage {
            seq: 1,
            name: "OR_ABI-L2-CMIPF-M6C13_G19_s20251772230205.lrit".to_owned(),
            satellite: Some("GOES-19".to_owned()),
            product: "Full disk".to_owned(),
            band: Some(13),
            width: 1356,
            lines: 1356,
            segments: 16,
            segments_expected: 16,
            complete: true,
            lines_missing: 2,
        }
        .present(&EventKind::named("goes_hrit"));
        assert_eq!(
            view.summary,
            "GOES-19 · Full disk · band 13 · 16/16 segments · 2 lines lost"
        );
        assert_eq!(view.field("Band"), count(Some(13)).as_ref());
        assert_eq!(view.entity, None);
    }
}

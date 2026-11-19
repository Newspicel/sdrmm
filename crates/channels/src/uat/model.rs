use sdrmm_decoder::default_true;
use sdrmm_wire::present::{EventPayload, Field, Presentation, count, fields, joined, text};
use sdrmm_wire::{Capability, EventContract, EventKind, Hue, KindSpec, KindView};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UatParams {
    /// Read FIS-B text from ground uplinks as well as aircraft.
    #[serde(default = "default_true")]
    pub fisb: bool,
}

impl Default for UatParams {
    fn default() -> Self {
        Self { fisb: true }
    }
}

impl sdrmm_wire::DecoderSettings for UatParams {
    const TYPE_ID: &'static str = "uat";
}

/// One FIS-B text report from a ground uplink: a METAR, TAF, PIREP, winds aloft, NOTAM,
/// AIRMET, SIGMET or airspace status.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FisbReport {
    pub product_id: u16,
    pub product: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issued: Option<String>,
    pub text: String,
}

impl EventPayload for FisbReport {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            headline: Some(fisb_headline(self)),
            entity: self.location.clone(),
            fields: fisb_fields(self),
            text: Some(self.text.clone()),
            ..Presentation::of(fisb_summary(self))
        }
    }
}

fn fisb_headline(r: &FisbReport) -> String {
    joined(
        [Some(r.product.clone()), r.location.clone()]
            .into_iter()
            .flatten(),
    )
}

fn fisb_fields(r: &FisbReport) -> Vec<Field> {
    fields(&[
        ("product", "Product", text(Some(&r.product))),
        ("location", "Location", text(r.location.as_deref())),
        ("issued", "Issued", text(r.issued.as_deref())),
        ("product_id", "Product ID", count(Some(r.product_id))),
        ("text", "Text", text(Some(&r.text))),
    ])
}

fn fisb_summary(r: &FisbReport) -> String {
    joined(
        [
            Some(r.product.clone()),
            r.location.clone(),
            r.issued.clone(),
            Some(r.text.clone()),
        ]
        .into_iter()
        .flatten(),
    )
}

pub(crate) fn fisb_kind() -> KindSpec {
    KindSpec::of::<FisbReport>("FIS-B", "Reports", Hue::Amber)
        .caps(&[Capability::Log, Capability::Text])
        .view(KindView::table(&["product", "location", "issued", "text"]))
        .build()
}

impl EventContract for FisbReport {
    const KIND: EventKind = EventKind::named("fisb");
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::DecoderEvent;

    fn metar() -> FisbReport {
        FisbReport {
            product_id: 413,
            product: "METAR".to_owned(),
            location: Some("KSPI".to_owned()),
            issued: Some("070652Z".to_owned()),
            text: "AUTO 36014KT 10SM -RA".to_owned(),
        }
    }

    #[test]
    fn a_report_reads_as_its_product_place_and_text() {
        let view = metar().present(&FisbReport::KIND);
        assert_eq!(view.headline.as_deref(), Some("METAR · KSPI"));
        assert_eq!(view.entity.as_deref(), Some("KSPI"));
        assert_eq!(
            view.summary,
            "METAR · KSPI · 070652Z · AUTO 36014KT 10SM -RA"
        );
        assert_eq!(view.text.as_deref(), Some("AUTO 36014KT 10SM -RA"));
    }

    #[test]
    fn the_report_table_names_fields_a_report_shows() {
        sdrmm_decoder_test_support::assert_views_hold(&fisb_kind(), &[DecoderEvent::of(&metar())]);
    }
}

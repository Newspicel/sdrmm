use sdrmm_wire::{Capability, EventContract, Hue, KindSpec, KindView};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::count;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::flag;
use sdrmm_wire::present::joined;
use sdrmm_wire::present::text;
use sdrmm_wire::present::text_of;
use sdrmm_wire::present::trimmed;
use sdrmm_wire::units::hertz;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RdsUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ps: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radiotext: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pty: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pty_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tp: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ta: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub music: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alt_freqs_hz: Vec<f64>,
    pub groups: u64,
    pub blocks: u64,
    pub block_errors: u64,
}

impl EventPayload for RdsUpdate {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            entity: trimmed(self.pi.as_deref()),
            label: trimmed(self.ps.as_deref()),
            fields: rds_fields(self),
            ..Presentation::of(rds_summary(self))
        }
    }
}

fn rds_fields(r: &RdsUpdate) -> Vec<Field> {
    let programme = r
        .pty_name
        .clone()
        .or_else(|| r.pty.map(|pty| format!("PTY {pty}")));
    let content = r.music.map(|music| if music { "music" } else { "speech" });
    let alternatives = r
        .alt_freqs_hz
        .iter()
        .map(|hz| hertz(*hz))
        .collect::<Vec<_>>()
        .join(", ");
    fields(&[
        ("pi", "PI", text(r.pi.as_deref())),
        ("station", "Station", text(r.ps.as_deref())),
        ("programme", "Programme", text(programme.as_deref())),
        ("traffic", "Traffic", flag(r.tp)),
        ("announcement", "Announcement", flag(r.ta)),
        ("content", "Content", content.and_then(text_of)),
        (
            "alt_frequencies",
            "Alt. frequencies",
            text(Some(&alternatives)),
        ),
        ("groups", "Groups", count(Some(r.groups as f64))),
        (
            "block_errors",
            "Block errors",
            count(Some(r.block_errors as f64)),
        ),
        ("radiotext", "Radiotext", text(r.radiotext.as_deref())),
    ])
}

fn rds_summary(r: &RdsUpdate) -> String {
    let mut parts = Vec::new();
    parts.extend(r.pi.as_ref().map(|pi| format!("PI {pi}")));
    parts.extend(r.ps.clone());
    parts.extend(r.pty_name.clone());
    parts.extend(r.radiotext.clone());
    joined(parts)
}

pub(crate) fn rds_kind() -> KindSpec {
    KindSpec::of::<RdsUpdate>("RDS", "Stations", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station])
        .sighting()
        .view(
            KindView::table(&[
                "pi",
                "station",
                "programme",
                "radiotext",
                "traffic",
                "announcement",
                "block_errors",
            ])
            .keyed("pi"),
        )
        .build()
}

impl EventContract for RdsUpdate {
    const KIND: EventKind = EventKind::named("rds");
}

#[cfg(test)]
mod model_tests {
    use sdrmm_wire::present::FieldValue;

    use super::*;

    fn field<'a>(view: &'a Presentation, label: &str) -> Option<&'a FieldValue> {
        view.fields
            .iter()
            .find(|field| field.label == label)
            .map(|field| &field.value)
    }

    #[test]
    fn rds_reads_as_fields_with_the_radiotext_last() {
        let view = serde_json::from_value::<RdsUpdate>(serde_json::json!({
            "groups": 100, "blocks": 402, "block_errors": 2, "pi": "D389", "ps": "RADIO 1 ",
            "pty_name": "Pop Music", "tp": true, "music": false,
            "alt_freqs_hz": [98_000_000.0, 100_500_000.0], "radiotext": "Now playing something",
        }))
        .unwrap()
        .present(&RdsUpdate::KIND);
        assert_eq!(field(&view, "PI"), text_of("D389").as_ref());
        assert_eq!(field(&view, "Station"), text_of("RADIO 1").as_ref());
        assert_eq!(field(&view, "Programme"), text_of("Pop Music").as_ref());
        assert_eq!(field(&view, "Traffic"), flag(Some(true)).as_ref());
        assert_eq!(field(&view, "Content"), text_of("speech").as_ref());
        assert_eq!(
            field(&view, "Alt. frequencies"),
            text_of("98 MHz, 100.5 MHz").as_ref()
        );
        assert_eq!(field(&view, "Groups"), count(Some(100)).as_ref());
        assert_eq!(field(&view, "Block errors"), count(Some(2)).as_ref());
        assert_eq!(
            view.fields.last().map(|field| field.label.as_str()),
            Some("Radiotext")
        );
    }

    #[test]
    fn the_station_table_names_fields_an_update_shows() {
        let update = RdsUpdate {
            pi: Some("D3C3".to_owned()),
            ps: Some("RADIO 1".to_owned()),
            radiotext: Some("Now playing".to_owned()),
            pty_name: Some("Pop Music".to_owned()),
            tp: Some(true),
            ta: Some(false),
            groups: 100,
            blocks: 400,
            block_errors: 3,
            ..RdsUpdate::default()
        };
        crate::testutil::assert_views_hold(&rds_kind(), &[sdrmm_wire::DecoderEvent::of(&update)]);
    }
}

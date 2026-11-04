use sdrmm_wire::{Capability, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::flag;
use sdrmm_wire::present::joined;
use sdrmm_wire::present::repaired;
use sdrmm_wire::present::text;
use sdrmm_wire::present::text_of;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavtexParams {
    #[serde(default)]
    pub invert: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NavtexMessage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<char>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<char>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<u8>,
    pub text: String,
    pub errors_corrected: u32,
    pub complete: bool,
}

impl NavtexMessage {
    #[must_use]
    pub fn header(&self) -> Option<String> {
        let (station, subject, serial) = (self.station?, self.subject?, self.serial?);
        Some(format!("{station}{subject}{serial:02}"))
    }

    #[must_use]
    pub fn subject_name(subject: char) -> Option<&'static str> {
        Some(match subject.to_ascii_uppercase() {
            'A' => "Navigational warning",
            'B' => "Meteorological warning",
            'C' => "Ice report",
            'D' => "Search and rescue / piracy",
            'E' => "Meteorological forecast",
            'F' => "Pilot service",
            'G' => "AIS",
            'H' => "LORAN",
            'J' => "SATNAV",
            'K' => "Other electronic navaid",
            'L' => "Navigational warning (additional)",
            'T' => "Test transmission",
            'V' => "Notice to fishermen",
            'W' => "Environmental",
            'X' | 'Y' => "Special service",
            'Z' => "No message on hand",
            _ => return None,
        })
    }
}

impl EventPayload for NavtexMessage {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            entity: self.station.map(String::from),
            fields: navtex_fields(self),
            ..Presentation::of(navtex_summary(self))
        }
    }
}

fn navtex_fields(n: &NavtexMessage) -> Vec<Field> {
    let subject = n
        .subject_name
        .clone()
        .or_else(|| n.subject.map(String::from));
    fields(&[
        ("Header", text(n.header().as_deref())),
        ("Station", n.station.and_then(text_of)),
        ("Subject", text(subject.as_deref())),
        (
            "Serial",
            n.serial.and_then(|serial| text_of(format!("{serial:02}"))),
        ),
        ("Complete", flag(Some(n.complete))),
        ("Repaired", repaired(n.errors_corrected)),
        ("Text", text(Some(&n.text))),
    ])
}

fn navtex_summary(n: &NavtexMessage) -> String {
    let mut parts = Vec::new();
    if let Some(id) = n.header() {
        parts.push(id);
    }
    if let Some(subject) = &n.subject_name {
        parts.push(subject.clone());
    }
    parts.push(n.text.replace('\n', " ").trim().to_owned());
    parts.retain(|p| !p.is_empty());
    joined(parts)
}

pub(crate) fn navtex_kind() -> KindSpec {
    KindSpec::of::<NavtexMessage>("NAVTEX", "Stations", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station])
        .sighting()
        .build()
}

impl EventContract for NavtexMessage {
    const KIND: EventKind = EventKind::named("navtex");
}

impl sdrmm_wire::DecoderSettings for NavtexParams {
    const TYPE_ID: &'static str = "navtex";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::present::count;

    #[test]
    fn navtex_subject_names_match_the_standard_table() {
        const NAMED: [(char, &str); 16] = [
            ('A', "Navigational warning"),
            ('B', "Meteorological warning"),
            ('C', "Ice report"),
            ('D', "Search and rescue / piracy"),
            ('E', "Meteorological forecast"),
            ('F', "Pilot service"),
            ('G', "AIS"),
            ('H', "LORAN"),
            ('J', "SATNAV"),
            ('K', "Other electronic navaid"),
            ('L', "Navigational warning (additional)"),
            ('T', "Test transmission"),
            ('V', "Notice to fishermen"),
            ('W', "Environmental"),
            ('X', "Special service"),
            ('Y', "Special service"),
        ];
        for (subject, name) in NAMED {
            assert_eq!(
                NavtexMessage::subject_name(subject),
                Some(name),
                "{subject}"
            );
        }
        assert_eq!(NavtexMessage::subject_name('Z'), Some("No message on hand"));
        for unassigned in ['I', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'U'] {
            assert_eq!(
                NavtexMessage::subject_name(unassigned),
                None,
                "{unassigned} is unassigned and must stay unnamed"
            );
        }
    }

    #[test]
    fn navtex_header_needs_the_whole_group() {
        let mut msg = NavtexMessage {
            station: Some('D'),
            subject: Some('A'),
            serial: Some(7),
            ..NavtexMessage::default()
        };
        assert_eq!(msg.header().as_deref(), Some("DA07"));
        msg.serial = None;
        assert_eq!(msg.header(), None);
    }

    #[test]
    fn navtex_keeps_its_text_and_the_header_it_came_under() {
        let view = serde_json::from_value::<NavtexMessage>(serde_json::json!({
            "station": "D", "subject": "A", "subject_name": "Navigational warning",
            "serial": 7, "text": "GALE WARNING\nGERMAN BIGHT", "errors_corrected": 3,
            "complete": false,
        }))
        .unwrap()
        .present(&EventKind::named("navtex"));
        assert_eq!(view.field("Header"), text(Some("DA07")).as_ref());
        assert_eq!(
            view.field("Subject"),
            text(Some("Navigational warning")).as_ref()
        );
        assert_eq!(view.field("Serial"), text(Some("07")).as_ref());
        assert_eq!(view.field("Complete"), flag(Some(false)).as_ref());
        assert_eq!(view.field("Repaired"), count(Some(3)).as_ref());
        assert_eq!(
            view.field("Text"),
            text(Some("GALE WARNING\nGERMAN BIGHT")).as_ref()
        );
    }
}

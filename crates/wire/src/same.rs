use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    decode::EventKind,
    present::{EventPayload, Field, Presentation, fields, joined, text, text_of},
};

pub mod codes;
pub mod header;

pub const SAME_BANDWIDTH_HZ: f64 = 12_500.0;
pub const MIN_SAME_BANDWIDTH_HZ: f64 = 6_000.0;
pub const MAX_SAME_BANDWIDTH_HZ: f64 = 25_000.0;
pub const SAME_BAUD: f64 = 3_125.0 / 6.0;
pub const SAME_MARK_HZ: f64 = SAME_BAUD * 4.0;
pub const SAME_SPACE_HZ: f64 = SAME_BAUD * 3.0;
pub const SAME_BURSTS: u8 = 3;
pub const MAX_SAME_LOCATIONS: usize = 31;

pub use header::{MAX_HEADER_LEN, parse};

fn default_same_bandwidth_hz() -> f64 {
    SAME_BANDWIDTH_HZ
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SameParams {
    #[serde(default = "default_same_bandwidth_hz")]
    pub bandwidth_hz: f64,
}

impl Default for SameParams {
    fn default() -> Self {
        Self {
            bandwidth_hz: default_same_bandwidth_hz(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SameLocation {
    pub part: u8,
    pub state: u8,
    pub county: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl SameLocation {
    #[must_use]
    pub fn code(&self) -> String {
        format!("{}{:02}{:03}", self.part, self.state, self.county)
    }

    #[must_use]
    pub fn label(&self) -> String {
        self.name.clone().unwrap_or_else(|| self.code())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SameAlert {
    pub originator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub originator_name: Option<String>,
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_name: Option<String>,
    pub locations: Vec<SameLocation>,
    pub valid_minutes: u32,
    pub issued_day: u16,
    pub issued_hour: u8,
    pub issued_minute: u8,
    pub station: String,
}

impl SameAlert {
    #[must_use]
    pub fn headline(&self) -> String {
        self.event_name
            .clone()
            .unwrap_or_else(|| self.event.clone())
    }

    #[must_use]
    pub fn area(&self) -> String {
        let mut names = self.locations.iter().map(SameLocation::label);
        let Some(first) = names.next() else {
            return String::new();
        };
        match names.count() {
            0 => first,
            more => format!("{first} +{more}"),
        }
    }

    #[must_use]
    pub fn issued(&self) -> String {
        format!(
            "day {} {:02}:{:02} UTC",
            self.issued_day, self.issued_hour, self.issued_minute
        )
    }

    #[must_use]
    pub fn valid_for(&self) -> String {
        let (hours, minutes) = (self.valid_minutes / 60, self.valid_minutes % 60);
        match (hours, minutes) {
            (0, minutes) => format!("{minutes} min"),
            (hours, 0) => format!("{hours} h"),
            (hours, minutes) => format!("{hours} h {minutes} min"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "header", rename_all = "snake_case")]
pub enum SameBody {
    Alert(SameAlert),
    EndOfMessage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SameMessage {
    pub body: SameBody,
    pub copies: u8,
    pub raw: String,
}

impl SameMessage {
    #[must_use]
    pub fn station(&self) -> Option<&str> {
        match &self.body {
            SameBody::Alert(alert) => Some(&alert.station),
            SameBody::EndOfMessage => None,
        }
    }

    #[must_use]
    pub fn summary(&self) -> String {
        match &self.body {
            SameBody::Alert(alert) => joined(
                [alert.headline(), alert.area(), alert.station.clone()]
                    .into_iter()
                    .filter(|part| !part.is_empty()),
            ),
            SameBody::EndOfMessage => "End of message".to_owned(),
        }
    }
}

impl EventPayload for SameMessage {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            entity: self.station().map(str::to_owned),
            fields: same_fields(self),
            ..Presentation::of(self.summary())
        }
    }
}

fn same_fields(message: &SameMessage) -> Vec<Field> {
    let shown = match &message.body {
        SameBody::Alert(alert) => alert_fields(alert),
        SameBody::EndOfMessage => fields(&[("Header", text_of("NNNN (end of message)"))]),
    };
    [
        shown,
        fields(&[
            (
                "Bursts",
                text_of(format!("{}/{SAME_BURSTS}", message.copies)),
            ),
            ("Raw", text(Some(&message.raw))),
        ]),
    ]
    .concat()
}

fn alert_fields(alert: &SameAlert) -> Vec<Field> {
    let areas = alert
        .locations
        .iter()
        .map(|location| format!("{} ({})", location.label(), location.code()))
        .collect::<Vec<_>>()
        .join("\n");
    fields(&[
        ("Event", text_of(alert.headline())),
        ("Event code", text_of(&alert.event)),
        (
            "Originator",
            text_of(
                alert
                    .originator_name
                    .clone()
                    .unwrap_or_else(|| alert.originator.clone()),
            ),
        ),
        ("Originator code", text_of(&alert.originator)),
        ("Areas", text(Some(&areas))),
        ("Valid for", text_of(alert.valid_for())),
        ("Issued", text_of(alert.issued())),
        ("Station", text_of(&alert.station)),
    ])
}

#[cfg(test)]
mod tests;

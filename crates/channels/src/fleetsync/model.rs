use serde::{Deserialize, Serialize};

use sdrmm_wire::present::{
    EventPayload, Presentation, Unit, count, fields, number, only_if, text, text_of,
};
use sdrmm_wire::{Capability, EventContract, EventKind, Hue, KindSpec};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FleetSyncParams {}

impl sdrmm_wire::DecoderSettings for FleetSyncParams {
    const TYPE_ID: &'static str = "fleetsync";
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FleetSyncVersion {
    One,
    Two,
}

impl FleetSyncVersion {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::One => "FleetSync I",
            Self::Two => "FleetSync II",
        }
    }
}

/// One `FleetSync` message. Fleets and units read as the radios show them: fleet 100 to 354,
/// unit 1000 to 5094. `None` addresses every fleet or unit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetSyncMessage {
    pub version: FleetSyncVersion,
    pub baud: u32,
    pub flags: u8,
    pub command: u8,
    pub from_fleet: Option<u16>,
    pub from_unit: Option<u16>,
    pub to_fleet: Option<u16>,
    pub to_unit: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

const TEXT: u8 = 0x42;
const ANI: u8 = 0x80;
const STATUS: u8 = 0x40;
const ACK: u8 = 0x20;
const GPS: u8 = 0x02;
/// Cleared in the flags of the ANI a radio sends as its key drops.
const KEYED: u8 = 0x02;

impl FleetSyncMessage {
    /// What the command bits say the message is, when they name it.
    #[must_use]
    pub const fn known_kind(&self) -> Option<&'static str> {
        Some(match self.command {
            TEXT => "Message",
            command if command & ACK != 0 => "Ack",
            command if command & GPS != 0 => "GPS",
            command if command & STATUS != 0 => "Status",
            command if command & ANI != 0 => "ANI",
            _ => return None,
        })
    }

    #[must_use]
    pub fn kind(&self) -> String {
        self.known_kind()
            .map_or_else(|| format!("Command {:02X}", self.command), str::to_owned)
    }

    /// The status number, sent in place of the flags.
    #[must_use]
    pub fn status(&self) -> Option<u8> {
        matches!(self.known_kind(), Some("Status")).then_some(self.flags >> 1)
    }

    /// For an ANI, whether it marks the end of the transmission rather than its start.
    #[must_use]
    pub fn end(&self) -> Option<bool> {
        matches!(self.known_kind(), Some("ANI")).then_some(self.flags & KEYED == 0)
    }

    #[must_use]
    pub fn sender(&self) -> String {
        address(self.from_fleet, self.from_unit)
    }

    #[must_use]
    pub fn receiver(&self) -> String {
        address(self.to_fleet, self.to_unit)
    }
}

fn address(fleet: Option<u16>, unit: Option<u16>) -> String {
    let part = |id: Option<u16>| id.map_or_else(|| "All".to_owned(), |id| id.to_string());
    format!("{}-{}", part(fleet), part(unit))
}

impl EventPayload for FleetSyncMessage {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let sender = self.sender();
        let kind = self.kind();
        let edge = self.end().map(|end| if end { "End" } else { "Start" });
        Presentation {
            entity: Some(sender.clone()),
            text: self.text.clone(),
            fields: fields(&[
                ("from", "From", text(Some(&sender))),
                ("to", "To", text_of(self.receiver())),
                ("type", "Type", text(Some(&kind))),
                ("ptt", "PTT", text(edge)),
                ("status", "Status", count(self.status())),
                ("version", "Version", text_of(self.version.label())),
                ("baud", "Baud", number(Some(self.baud), Unit::None, 0)),
                ("truncated", "Truncated", only_if(self.truncated)),
            ]),
            verified: Some(true),
            ..Presentation::of(format!("{sender} · {kind}"))
        }
    }
}

impl EventContract for FleetSyncMessage {
    const KIND: EventKind = EventKind::named("fleetsync");
}

pub(crate) fn fleetsync_kind() -> KindSpec {
    KindSpec::of::<FleetSyncMessage>("FleetSync", "Radios", Hue::Teal)
        .caps(&[Capability::Log, Capability::Station, Capability::Text])
        .sighting()
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(flags: u8, command: u8) -> FleetSyncMessage {
        FleetSyncMessage {
            version: FleetSyncVersion::One,
            baud: 1_200,
            flags,
            command,
            from_fleet: Some(107),
            from_unit: Some(1772),
            to_fleet: Some(107),
            to_unit: None,
            text: None,
            truncated: false,
        }
    }

    #[test]
    fn an_ani_reads_start_and_end() {
        let start = message(0xFE, 0x80).present(&FleetSyncMessage::KIND);
        assert_eq!(start.summary, "107-1772 · ANI");
        assert_eq!(start.field("PTT"), text(Some("Start")).as_ref());
        assert_eq!(start.field("To"), text(Some("107-All")).as_ref());
        let end = message(0xFC, 0x80).present(&FleetSyncMessage::KIND);
        assert_eq!(end.field("PTT"), text(Some("End")).as_ref());
    }

    #[test]
    fn a_status_carries_its_number_in_the_flags() {
        let status = message(0x0A, 0x40);
        assert_eq!(status.kind(), "Status");
        assert_eq!(status.status(), Some(5));
        assert_eq!(status.end(), None);
    }

    #[test]
    fn an_unknown_command_shows_its_byte() {
        assert_eq!(message(0, 0x04).kind(), "Command 04");
    }
}

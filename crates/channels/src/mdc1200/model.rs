use serde::{Deserialize, Serialize};

use sdrmm_wire::present::{
    EventPayload, Presentation, count, fields, only_if, repaired, text, text_of,
};
use sdrmm_wire::{Capability, EventContract, EventKind, Hue, KindSpec};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MdcParams {}

impl sdrmm_wire::DecoderSettings for MdcParams {
    const TYPE_ID: &'static str = "mdc1200";
}

/// One MDC-1200 packet. A double packet carries the four bytes of its second block in
/// `extra`; `truncated` marks one whose second block was lost.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MdcPacket {
    pub unit: u16,
    pub op: u8,
    pub arg: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<[u8; 4]>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
    #[serde(default)]
    pub repaired: u32,
}

impl MdcPacket {
    /// What the op means, after the published op and argument table.
    #[must_use]
    pub fn meaning(&self) -> Option<&'static str> {
        Some(match (self.op, self.arg) {
            (0x01, arg) if arg & 0x80 != 0 => "PTT ID",
            (0x01, _) => "PTT ID end",
            (0x00, _) => "Emergency",
            (0x20, _) => "Emergency ack",
            (0x11, _) => "Remote monitor",
            (0x22, _) => "Status request",
            (0x06, _) => "Status reply",
            (0x46, _) => "Status",
            (0x07 | 0x47, _) => "Message",
            (0x23, _) => "Ack",
            (0x2B, 0x0C) => "Radio enable",
            (0x2B, _) => "Radio inhibit",
            (0x0B, 0x0C) => "Radio enable ack",
            (0x0B, _) => "Radio inhibit ack",
            (0x40 | 0x41, _) => "Request to talk",
            (0x63, _) => "Radio check",
            (0x30, _) => "Radio check ack",
            (0x35, _) => self.call_meaning(),
            _ => return None,
        })
    }

    fn call_meaning(&self) -> &'static str {
        match self.extra.map(|extra| extra[0]) {
            Some(0x81 | 0x83) => "Call alert",
            Some(0x80 | 0x82) => "Selective call",
            Some(0xA0) => "Call alert ack",
            _ => "Call",
        }
    }

    /// The status or message number an op carries in its argument.
    #[must_use]
    pub fn number(&self) -> Option<u8> {
        matches!(self.op, 0x06 | 0x46 | 0x07 | 0x47).then_some(self.arg)
    }

    /// The calling unit of a double packet; the first block names the called one.
    #[must_use]
    pub fn caller(&self) -> Option<u16> {
        self.extra
            .map(|extra| u16::from_be_bytes([extra[2], extra[3]]))
    }

    fn label(&self) -> String {
        self.meaning().map_or_else(
            || format!("Op {:02X} {:02X}", self.op, self.arg),
            str::to_owned,
        )
    }
}

impl EventPayload for MdcPacket {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let unit = format!("{:04X}", self.unit);
        let caller = self.caller().map(|caller| format!("{caller:04X}"));
        Presentation {
            entity: Some(caller.clone().unwrap_or_else(|| unit.clone())),
            fields: fields(&[
                ("unit", "Unit", text(Some(&unit))),
                ("meaning", "Meaning", text_of(self.label())),
                ("number", "Number", count(self.number())),
                ("from", "From", text(caller.as_deref())),
                ("op", "Op", text_of(format!("{:02X}", self.op))),
                ("arg", "Arg", text_of(format!("{:02X}", self.arg))),
                ("truncated", "Second block lost", only_if(self.truncated)),
                ("repaired", "Repaired", repaired(self.repaired)),
            ]),
            verified: Some(true),
            ..Presentation::of(format!("{unit} · {}", self.label()))
        }
    }
}

impl EventContract for MdcPacket {
    const KIND: EventKind = EventKind::named("mdc1200");
}

pub(crate) fn mdc_kind() -> KindSpec {
    KindSpec::of::<MdcPacket>("MDC-1200", "Radios", Hue::Orange)
        .caps(&[Capability::Log, Capability::Station])
        .sighting()
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(op: u8, arg: u8, extra: Option<[u8; 4]>) -> MdcPacket {
        MdcPacket {
            unit: 0x5678,
            op,
            arg,
            extra,
            truncated: false,
            repaired: 0,
        }
    }

    #[test]
    fn ptt_id_reads_start_and_end() {
        assert_eq!(packet(0x01, 0x80, None).meaning(), Some("PTT ID"));
        assert_eq!(packet(0x01, 0x00, None).meaning(), Some("PTT ID end"));
    }

    #[test]
    fn a_call_alert_names_its_caller() {
        let call = packet(0x35, 0x89, Some([0x83, 0x0D, 0x12, 0x34]));
        assert_eq!(call.meaning(), Some("Call alert"));
        assert_eq!(call.caller(), Some(0x1234));
        let view = call.present(&MdcPacket::KIND);
        assert_eq!(view.summary, "5678 · Call alert");
        assert_eq!(view.entity.as_deref(), Some("1234"));
        assert_eq!(view.field("From"), text(Some("1234")).as_ref());
    }

    #[test]
    fn a_status_shows_its_number() {
        let view = packet(0x46, 0x03, None).present(&MdcPacket::KIND);
        assert_eq!(view.field("Number"), count(Some(3)).as_ref());
    }

    #[test]
    fn an_unknown_op_shows_its_bytes() {
        let view = packet(0x5A, 0x01, None).present(&MdcPacket::KIND);
        assert_eq!(view.summary, "5678 · Op 5A 01");
    }
}

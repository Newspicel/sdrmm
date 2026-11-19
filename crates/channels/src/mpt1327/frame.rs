//! A decoded message as the digital voice frame the trunk follower and the event views read.

use sdrmm_wire::{DvFrame, DvFrameKind};

use super::message::{Address, Message};

pub(crate) const MODE: &str = "MPT 1327";
pub(crate) const PROTOCOL: &str = "mpt1327";
pub(crate) const LABEL: &str = "MPT 1327 trunking";

const ANNOUNCE_CONTROL: u8 = 0b00000;
const WITHDRAW_CONTROL: u8 = 0b00001;
const ADJACENT_CONTROL: u8 = 0b00100;

/// One message as heard: its address codeword, the data codewords that came with it, and where.
pub(crate) struct Heard<'a> {
    pub message: Message,
    pub info: u64,
    pub data: &'a [u64],
    pub errors: u32,
    pub system: Option<u16>,
    pub control: bool,
}

pub(crate) fn dv_frame(heard: &Heard<'_>) -> DvFrame {
    let message = heard.message;
    let mut frame = DvFrame {
        trunk_protocol: Some(PROTOCOL.to_owned()),
        trunk_label: Some(LABEL.to_owned()),
        control_channel: Some(heard.control),
        crc_verified: Some(true),
        system_id: heard.system,
        opcode: Some(message.mnemonic().to_owned()),
        text: Some(describe(message)),
        data: Some(raw(heard.info, heard.data)),
        errors_corrected: heard.errors,
        ..DvFrame::new(MODE, kind(message))
    };
    let (to, from) = parties(message);
    if let Some(to) = to {
        frame.destination = Some(to.number());
        frame.destination_call = Some(to.label());
    }
    if let Some(from) = from {
        frame.source = Some(from.number());
        frame.source_call = Some(from.label());
    }
    frame.channel = channel(message);
    frame
}

/// A data call's grant moves no voice, so it reads as data and the trunk follower leaves it.
fn kind(message: Message) -> DvFrameKind {
    match message {
        Message::Gtc { data: true, .. } | Message::ShortData { .. } => DvFrameKind::Data,
        _ => DvFrameKind::Control,
    }
}

fn parties(message: Message) -> (Option<Address>, Option<Address>) {
    match message {
        Message::Gtc {
            called, calling, ..
        }
        | Message::Ack {
            called, calling, ..
        }
        | Message::Ahoy {
            called, calling, ..
        }
        | Message::ShortData {
            called, calling, ..
        } => (Some(called), Some(calling)),
        Message::Maint { unit, .. }
        | Message::SingleAddress { unit }
        | Message::StandardData { unit } => (Some(unit), None),
        _ => (None, None),
    }
}

fn channel(message: Message) -> Option<u16> {
    match message {
        Message::Gtc { channel, .. } | Message::Maint { channel, .. } => Some(channel),
        Message::Clear { control, .. } | Message::Move { control, .. } if control > 0 => {
            Some(control)
        }
        Message::Broadcast {
            definition: ANNOUNCE_CONTROL | WITHDRAW_CONTROL | ADJACENT_CONTROL,
            channel,
            ..
        } => Some(channel),
        _ => None,
    }
}

fn raw(info: u64, data: &[u64]) -> String {
    std::iter::once(info)
        .chain(data.iter().copied())
        .map(|word| format!("{word:012X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn describe(message: Message) -> String {
    match message {
        Message::Gtc { channel, data, .. } => {
            let call = if data { "Data call" } else { "Call" };
            format!("{call} to channel {channel}")
        }
        Message::Aloha { function, .. } => aloha(function).to_owned(),
        Message::Ack { function, .. } => acknowledgement(function).to_owned(),
        Message::Ahoy { function, .. } => ahoy(function).to_owned(),
        Message::Mark { system, .. } => format!("Control channel of system {system:04X}"),
        Message::Maint { operation, .. } => maintenance(operation).to_owned(),
        Message::Clear { channel, .. } => format!("Channel {channel} cleared"),
        Message::Move { .. } => "Move to another control channel".to_owned(),
        Message::Broadcast {
            definition, system, ..
        } => broadcast(definition, system),
        Message::ShortData { codewords, .. } => format!("Short data, {codewords} codewords"),
        Message::SingleAddress { .. } => "Single address message".to_owned(),
        Message::StandardData { .. } => "Standard data".to_owned(),
        Message::Unassigned { category, .. } => format!("Category {category:03b} message"),
    }
}

const fn aloha(function: u8) -> &'static str {
    match function {
        0 => "Aloha",
        1 => "Aloha, no standard data",
        2 => "Aloha, no simple calls",
        3 => "Aloha, emergencies only",
        4 => "Aloha, registration or emergency",
        5 => "Aloha, no registration",
        6 => "Aloha, fall-back mode",
        _ => "Aloha, reserved",
    }
}

const fn acknowledgement(function: u8) -> &'static str {
    match function {
        0 => "Acknowledged",
        1 => "More signalling to follow",
        2 => "Call queued",
        3 => "Rejected",
        4 => "Called unit unavailable",
        5 => "Emergency acknowledged",
        6 => "Try another address",
        _ => "Call back",
    }
}

const fn ahoy(function: u8) -> &'static str {
    match function {
        0 => "Availability check",
        2 => "Cancel alert",
        5 => "Presence check",
        6 => "Status message",
        7 => "Short data invitation",
        _ => "Ahoy, reserved",
    }
}

const fn maintenance(operation: u8) -> &'static str {
    match operation {
        0 => "Pressel on",
        1 => "Pressel off",
        2 => "Call in progress",
        3 => "Disconnect",
        6 => "Clear down other units",
        7 => "Disable transmission",
        _ => "Call maintenance",
    }
}

fn broadcast(definition: u8, system: u16) -> String {
    let what = match definition {
        ANNOUNCE_CONTROL => "Control channel announced",
        WITHDRAW_CONTROL => "Control channel withdrawn",
        0b00010 => "Call maintenance parameters",
        0b00011 => "Registration parameters",
        ADJACENT_CONTROL => "Adjacent site control channel",
        0b00101 => "Vote now",
        _ => "Broadcast",
    };
    format!("{what}, system {system:04X}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard(message: Message) -> DvFrame {
        dv_frame(&Heard {
            message,
            info: 0x8000_0401_B800,
            data: &[0x1234],
            errors: 1,
            system: Some(0x5009),
            control: true,
        })
    }

    #[test]
    fn a_grant_carries_the_channel_and_both_parties() {
        let frame = heard(Message::Gtc {
            called: Address {
                prefix: 10,
                ident: 4105,
            },
            calling: Address {
                prefix: 10,
                ident: 77,
            },
            channel: 123,
            data: false,
        });
        assert_eq!(frame.kind, DvFrameKind::Control);
        assert_eq!(frame.channel, Some(123));
        assert_eq!(frame.destination, Some(10 * 8192 + 4105));
        assert_eq!(frame.source_call.as_deref(), Some("10/77"));
        assert_eq!(frame.opcode.as_deref(), Some("GTC"));
        assert_eq!(frame.system_id, Some(0x5009));
        assert_eq!(frame.data.as_deref(), Some("80000401B800 000000001234"));
    }

    #[test]
    fn a_data_grant_is_not_signalling_a_voice_call() {
        let frame = heard(Message::Gtc {
            called: Address {
                prefix: 1,
                ident: 2,
            },
            calling: Address {
                prefix: 1,
                ident: 3,
            },
            channel: 4,
            data: true,
        });
        assert_eq!(frame.kind, DvFrameKind::Data);
    }

    #[test]
    fn a_maintenance_broadcast_names_no_channel() {
        let frame = heard(Message::Broadcast {
            definition: 2,
            system: 0x5009,
            channel: 516,
            adjacent_site: 4,
        });
        assert_eq!(frame.channel, None);
        assert_eq!(
            frame.text.as_deref(),
            Some("Call maintenance parameters, system 5009")
        );
    }
}

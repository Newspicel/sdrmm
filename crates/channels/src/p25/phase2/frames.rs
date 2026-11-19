use sdrmm_crypto::hex_string;
use sdrmm_wire::{CodeStyle, DvFrame, DvFrameKind};

use super::super::decode_p25_vendor;
use super::super::trunk::BandPlan;
use super::mac::{MacPdu, Message, PduKind, messages};
use super::scrambler::Identity;

pub(crate) const MODE: &str = "P25 Phase 2";

const CLEAR_ALGORITHM: u8 = 0x80;
const MOTOROLA: u8 = 0x90;
const SERVICE_EMERGENCY: u8 = 0x80;
const SERVICE_ENCRYPTED: u8 = 0x40;

/// Where a burst was heard: its logical channel, as TS1 or TS2, and the site when known.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Heard {
    pub(crate) slot: u8,
    pub(crate) identity: Option<Identity>,
    pub(crate) corrected: u32,
}

impl Heard {
    pub(crate) fn frame(self, kind: DvFrameKind, opcode: &str) -> DvFrame {
        let mut frame = DvFrame {
            code_style: CodeStyle::Nac,
            slot: Some(self.slot),
            crc_verified: Some(true),
            errors_corrected: self.corrected,
            opcode: Some(opcode.to_owned()),
            ..DvFrame::new(MODE, kind)
        };
        if let Some(identity) = self.identity {
            frame.color_code = Some(identity.nac);
            frame.network_id = Some(identity.wacn);
            frame.system_id = Some(identity.system);
        }
        frame
    }
}

/// What a MAC PDU says about the call on its channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CallState {
    /// Whether the call is encrypted.
    Encrypted(bool),
    Ended,
}

pub(crate) fn pdu_frames(
    pdu: &MacPdu,
    heard: Heard,
    bands: &mut BandPlan,
    out: &mut Vec<DvFrame>,
) -> Option<CallState> {
    let bytes = pdu.bytes();
    match pdu.kind() {
        PduKind::Ptt => {
            let frame = push_to_talk(bytes, heard);
            let state = frame.encrypted.map(CallState::Encrypted);
            out.push(frame);
            state
        }
        PduKind::EndPtt => {
            out.push(end_push_to_talk(bytes, heard));
            Some(CallState::Ended)
        }
        _ => {
            let mut state = None;
            for message in messages(pdu) {
                let start = out.len();
                match message {
                    Message::Known(message) => message_frames(message, heard, bands, out),
                    Message::Unsized(rest) => {
                        let mut frame = heard.frame(
                            DvFrameKind::Control,
                            &format!("MAC message {:02X}, length unknown", rest[0]),
                        );
                        frame.data = Some(hex_string(rest));
                        out.push(frame);
                    }
                }
                for frame in &out[start..] {
                    if frame.kind == DvFrameKind::Voice
                        && let Some(encrypted) = frame.encrypted
                    {
                        state = Some(CallState::Encrypted(encrypted));
                    }
                }
            }
            state
        }
    }
}

fn push_to_talk(bytes: &[u8], heard: Heard) -> DvFrame {
    let mut frame = heard.frame(DvFrameKind::Header, "push to talk");
    frame.message_indicator = Some(hex_string(&bytes[1..10]));
    frame.algorithm_id = Some(bytes[10]);
    frame.key_id = Some(u16_at(bytes, 11));
    frame.encrypted = Some(bytes[10] != CLEAR_ALGORITHM);
    frame.source = Some(u24_at(bytes, 13));
    frame.destination = Some(u32::from(u16_at(bytes, 16)));
    frame.group_call = Some(true);
    frame
}

fn end_push_to_talk(bytes: &[u8], heard: Heard) -> DvFrame {
    let mut frame = heard.frame(DvFrameKind::Terminator, "end push to talk");
    frame.source = Some(u24_at(bytes, 13));
    frame.destination = Some(u32::from(u16_at(bytes, 16)));
    frame.group_call = Some(true);
    frame
}

fn message_frames(message: &[u8], heard: Heard, bands: &mut BandPlan, out: &mut Vec<DvFrame>) {
    let opcode = message[0];
    if opcode >> 6 == 2 {
        out.push(vendor(message, heard));
        return;
    }
    if let Some(tsbk) = identifier_update(message) {
        bands.learn(&tsbk);
    }
    let first = out.len();
    match opcode {
        0x05 => {
            for entry in message[1..].as_chunks::<5>().0 {
                out.push(group_grant(
                    heard,
                    opcode,
                    entry[0],
                    u16_at(entry, 1),
                    u16_at(entry, 3),
                ));
            }
        }
        0x25 => {
            for entry in message[1..].as_chunks::<7>().0 {
                out.push(group_grant(
                    heard,
                    opcode,
                    entry[0],
                    u16_at(entry, 1),
                    u16_at(entry, 5),
                ));
            }
        }
        0x42 => {
            for entry in message[1..].as_chunks::<4>().0 {
                out.push(group_grant(
                    heard,
                    opcode,
                    0,
                    u16_at(entry, 0),
                    u16_at(entry, 2),
                ));
            }
        }
        _ => out.push(single(message, heard)),
    }
    for frame in &mut out[first..] {
        bands.place(frame);
    }
}

fn group_grant(heard: Heard, opcode: u8, service: u8, channel: u16, group: u16) -> DvFrame {
    let mut frame = heard.frame(DvFrameKind::Control, message_name(opcode));
    service_options(&mut frame, service);
    frame.channel = Some(channel);
    frame.destination = Some(u32::from(group));
    frame.group_call = Some(true);
    frame
}

fn single(message: &[u8], heard: Heard) -> DvFrame {
    let opcode = message[0];
    let kind = if matches!(opcode, 0x01 | 0x02 | 0x21 | 0x22) {
        DvFrameKind::Voice
    } else {
        DvFrameKind::Control
    };
    let mut frame = heard.frame(kind, message_name(opcode));
    let at16 = |index| u32::from(u16_at(message, index));
    match opcode {
        0x01 | 0x21 => {
            service_options(&mut frame, message[1]);
            frame.group_call = Some(true);
            frame.destination = Some(at16(2));
            frame.source = Some(u24_at(message, 4));
        }
        0x02 | 0x22 => {
            service_options(&mut frame, message[1]);
            frame.group_call = Some(false);
            frame.destination = Some(u24_at(message, 2));
            frame.source = Some(u24_at(message, 5));
        }
        0x40 => {
            service_options(&mut frame, message[1]);
            frame.group_call = Some(true);
            frame.channel = Some(u16_at(message, 2));
            frame.destination = Some(at16(4));
            frame.source = Some(u24_at(message, 6));
        }
        0x44 | 0x46 => {
            frame.group_call = Some(false);
            frame.channel = Some(u16_at(message, 1));
            frame.destination = Some(u24_at(message, 3));
            frame.source = Some(u24_at(message, 6));
        }
        0xC0 | 0xC3 => {
            service_options(&mut frame, message[1]);
            frame.group_call = Some(true);
            frame.channel = Some(u16_at(message, 2));
            frame.destination = Some(at16(6));
            if opcode == 0xC0 {
                frame.source = Some(u24_at(message, 8));
            }
        }
        0x7A | 0xFA | 0x7C | 0xFC => {
            frame.system_id = Some(u16_at(message, 2) & 0xFFF);
            frame.site_id = Some(u16::from(message[5]));
            frame.channel = Some(u16_at(message, 6));
            frame.data = Some(format!("RFSS {}", message[4]));
        }
        0x7B | 0xFB => {
            frame.network_id = Some(u24_at(message, 2) >> 4);
            frame.system_id = Some(u16_at(message, 4) & 0xFFF);
            frame.channel = Some(u16_at(message, 6));
        }
        0x31 => frame.destination = Some(u24_at(message, 2)),
        _ => frame.data = Some(hex_string(message)),
    }
    frame
}

fn service_options(frame: &mut DvFrame, options: u8) {
    frame.emergency = Some(options & SERVICE_EMERGENCY != 0);
    frame.encrypted = Some(options & SERVICE_ENCRYPTED != 0);
}

/// A manufacturer's message: its length and the vendor are in its second and third bytes.
fn vendor(message: &[u8], heard: Heard) -> DvFrame {
    let opcode = message[0];
    let mfid = message.get(1).copied().unwrap_or(0);
    let mut frame = heard.frame(DvFrameKind::Control, "");
    decode_p25_vendor(&mut frame, mfid);
    frame.data = Some(hex_string(message));
    let label = frame.vendor.map_or("vendor", |vendor| vendor.label());
    let regroup = match (mfid, opcode) {
        (MOTOROLA, 0x81) => Some("patch add"),
        (MOTOROLA, 0x89) => Some("patch delete"),
        _ => None,
    };
    let Some(name) = regroup.filter(|_| message.len() >= 5) else {
        frame.opcode = Some(format!("{label} MAC {opcode:02X}, unparsed"));
        return frame;
    };
    frame.opcode = Some(format!("{label} {name}"));
    frame.group_call = Some(true);
    frame.destination = Some(u32::from(u16_at(message, 3)));
    let mut groups: Vec<u16> = message[5..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    groups.dedup();
    let groups: Vec<String> = groups.iter().map(u16::to_string).collect();
    frame.text = Some(format!("TG {}", groups.join(", ")));
    frame
}

/// A channel identifier update as the Phase 1 trunking block that carries the same fields.
fn identifier_update(message: &[u8]) -> Option<[bool; 96]> {
    let tsbk_opcode = match message[0] {
        0x73 => 0x33,
        0x74 => 0x34,
        0x7D => 0x3D,
        _ => return None,
    };
    let fields = message.get(1..9)?;
    let mut bytes = [0u8; 12];
    bytes[0] = tsbk_opcode;
    bytes[2..10].copy_from_slice(fields);
    Some(std::array::from_fn(|bit| {
        bytes[bit / 8] >> (7 - bit % 8) & 1 == 1
    }))
}

fn u16_at(bytes: &[u8], index: usize) -> u16 {
    u16::from_be_bytes([
        bytes.get(index).copied().unwrap_or(0),
        bytes.get(index + 1).copied().unwrap_or(0),
    ])
}

fn u24_at(bytes: &[u8], index: usize) -> u32 {
    (0..3).fold(0u32, |value, offset| {
        value << 8 | u32::from(bytes.get(index + offset).copied().unwrap_or(0))
    })
}

fn message_name(opcode: u8) -> &'static str {
    match opcode {
        0x01 => "group voice channel user",
        0x02 => "unit-to-unit voice channel user",
        0x03 => "telephone interconnect voice channel user",
        0x05 => "group voice channel grant update multiple",
        0x11 => "indirect group paging",
        0x12 => "individual paging",
        0x21 => "group voice channel user extended",
        0x22 => "unit-to-unit voice channel user extended",
        0x25 => "group voice channel grant update multiple explicit",
        0x30 => "power control signal quality",
        0x31 => "MAC release",
        0x40 => "group voice channel grant",
        0x42 => "group voice channel grant update",
        0x44 => "unit-to-unit voice channel grant",
        0x45 => "unit-to-unit answer request",
        0x46 => "unit-to-unit voice channel grant update",
        0x48 => "telephone interconnect voice channel grant",
        0x49 => "telephone interconnect voice channel grant update",
        0x60 => "acknowledge response",
        0x61 => "queued response",
        0x64 => "extended function command",
        0x67 => "deny response",
        0x68 => "group affiliation response",
        0x6A => "group affiliation query",
        0x6B => "location registration response",
        0x6C => "unit registration response",
        0x6D => "unit registration command",
        0x6F => "deregistration acknowledge",
        0x70 => "synchronization broadcast",
        0x71 => "authentication demand",
        0x72 => "authentication response",
        0x73 => "channel identifier update TDMA",
        0x74 => "channel identifier update VHF/UHF",
        0x75 => "time and date announcement",
        0x76 => "roaming address command",
        0x77 => "roaming address update",
        0x78 => "system service broadcast",
        0x79 => "secondary control channel broadcast",
        0x7A | 0xFA => "RFSS status broadcast",
        0x7B | 0xFB => "network status broadcast",
        0x7C | 0xFC | 0xFE => "adjacent site status broadcast",
        0x7D => "channel identifier update",
        0xC0 => "group voice channel grant explicit",
        0xC3 => "group voice channel grant update explicit",
        0xC4 => "unit-to-unit voice channel grant extended",
        0xC6 => "unit-to-unit voice channel grant update extended",
        0xF3 => "channel identifier update TDMA extended",
        _ => "MAC message",
    }
}

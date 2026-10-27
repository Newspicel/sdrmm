use sdrmm_dsp::crc16_msb;
use sdrmm_wire::{IsmCoding, IsmIntegrity, IsmMessage};

use crate::ism::{
    bits::{Bits, Packed},
    device::{Device, Found, Outcome, ook, reading},
    slicer::Timing,
};

pub(super) const GENERIC: Device = ook(
    IsmCoding::Pwm,
    Timing::new(464, 1_404, 1_800).tolerance(200),
    generic,
)
.integrity(IsmIntegrity::Repeats);

const TRISTATE: [char; 4] = ['0', 'Z', 'X', '1'];

fn generic(bits: &mut Bits, found: &mut Found) -> Outcome {
    if bits.rows() == 0 || bits.len(0) != 25 {
        return Outcome::Skip;
    }
    let row = bits.row(0);
    let b = [!row[0], !row[1], !row[2], row[3]];
    if b[3] & 0x80 == 0 || (b[0] == 0 && b[1] == 0) || b[2] == 0 {
        return Outcome::Skip;
    }
    let full = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
    let tristate = (0..12)
        .rev()
        .map(|pair| TRISTATE[(full >> (pair * 2) & 3) as usize])
        .collect();
    Outcome::emit(
        found,
        IsmMessage {
            command: Some(u32::from(b[2])),
            tristate: Some(tristate),
            ..reading(
                "Generic-Remote",
                (u32::from(b[0]) << 8 | u32::from(b[1])).to_string(),
                &b,
                25,
            )
        },
    )
}

pub(super) const HONEYWELL: Device =
    ook(IsmCoding::Pcm, Timing::new(136, 136, 408), honeywell).integrity(IsmIntegrity::Crc);

const HONEYWELL_PREAMBLE: [u8; 3] = [0x55, 0x55, 0x56];
const TWO_GIG: [u8; 5] = [0x2, 0x4, 0x9, 0xa, 0xc];

fn honeywell(bits: &mut Bits, found: &mut Found) -> Outcome {
    let len = bits.len(0);
    if bits.rows() != 1 || len < 120 {
        return Outcome::Skip;
    }
    let (mut at, mut outcome) = (0, Outcome::Skip);
    loop {
        at = bits.search(0, at, &HONEYWELL_PREAMBLE, 24);
        if at + 24 >= len {
            return outcome;
        }
        let mut packed = Packed::new();
        bits.manchester(0, at + 24, 96, &mut packed);
        at += 1;
        if packed.len() < 48 {
            continue;
        }
        outcome = Outcome::Mic;
        let b = packed.bytes();
        let channel = b[0] >> 4;
        let id = u32::from(b[0] & 0x0f) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        let crc = u16::from_be_bytes([b[4], b[5]]);
        let poly = if TWO_GIG.contains(&channel) {
            0x8050
        } else {
            0x8005
        };
        if (id == 0 && crc == 0) || crc16_msb(poly, 0, &b[..4]) != crc {
            continue;
        }
        let event = b[3];
        return Outcome::emit(
            found,
            IsmMessage {
                channel: Some(channel.to_string()),
                battery_ok: Some(event & 0x08 == 0),
                contact_open: Some(event & 0x80 != 0),
                tamper: Some(event & 0x40 != 0),
                alarm: Some(event & 0x10 != 0),
                ..reading("Honeywell-Security", id.to_string(), &b[..6], 48)
            },
        );
    }
}

pub(super) const KERUI: Device = ook(
    IsmCoding::Pwm,
    Timing::new(420, 960, 9_900).gap(1_100).tolerance(160),
    kerui,
)
.integrity(IsmIntegrity::Repeats);

fn kerui(bits: &mut Bits, found: &mut Found) -> Outcome {
    let Some(index) = bits.repeated_row(9, 25) else {
        return Outcome::Skip;
    };
    let row = bits.row(index);
    if bits.len(index) != 25 || row[..3].iter().all(|&byte| byte == 0) {
        return Outcome::Skip;
    }
    let b = [!row[0], !row[1], !row[2]];
    let id = u32::from(b[0]) << 12 | u32::from(b[1]) << 4 | u32::from(b[2] >> 4);
    let command = b[2] & 0x0f;
    let base = IsmMessage {
        command: Some(u32::from(command)),
        ..reading("Kerui-Security", id.to_string(), &b, 25)
    };
    let message = match command {
        0xa | 0x5 => IsmMessage {
            alarm: Some(true),
            ..base
        },
        0xe => IsmMessage {
            contact_open: Some(true),
            ..base
        },
        0x7 => IsmMessage {
            contact_open: Some(false),
            ..base
        },
        0xb => IsmMessage {
            tamper: Some(true),
            ..base
        },
        0xf => IsmMessage {
            battery_ok: Some(false),
            ..base
        },
        _ => return Outcome::Skip,
    };
    Outcome::emit(found, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ism::bits::from_rows;

    const KERUI_OPEN: &str = "0111001111010000111100010";

    #[test]
    fn a_kerui_contact_frame_inverts_its_bits_and_reads_the_open_command() {
        let rows = [KERUI_OPEN; 9];
        let mut bits = from_rows(&rows);
        let mut found = Found::new();
        assert_eq!(kerui(&mut bits, &mut found), Outcome::Decoded);
        let [message] = found.as_slice() else {
            panic!("{found:?}");
        };
        assert_eq!(message.model, "Kerui-Security");
        assert_eq!(message.id, "574192");
        assert_eq!(message.command, Some(0x0e));
        assert_eq!(message.contact_open, Some(true));
        assert_eq!(message.raw, "8c2f0e");
        assert_eq!(message.bits, 25);
    }

    #[test]
    fn a_kerui_frame_without_enough_repeats_is_skipped() {
        let rows = [KERUI_OPEN; 8];
        let mut bits = from_rows(&rows);
        let mut found = Found::new();
        assert_eq!(kerui(&mut bits, &mut found), Outcome::Skip);
    }
}

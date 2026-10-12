use sdrmm_wire::{IsmCoding, IsmMessage};

use crate::ism::{
    bits::{Bits, Packed, reflect_nibbles},
    device::{Device, Found, Outcome, ook, reading},
    slicer::Timing,
};

pub(super) const OREGON: Device = ook(IsmCoding::Manchester, Timing::new(440, 0, 2_400), oregon);

const THGR122N: u16 = 0x1d20;
const THGR968: u16 = 0x1d30;
const THN132N: u16 = 0xec40;
const WGR800: [u16; 2] = [0x1984, 0x1994];
const THGR810: u16 = 0xf024;
const THGR810_MASK: u16 = 0xf0ff;
const V3_PREAMBLE: [u8; 2] = [0x00, 0x05];
const V3_BYTES: usize = 44;
const V2_MAX_BITS: usize = 173;

fn oregon(bits: &mut Bits, found: &mut Found) -> Outcome {
    let v2 = v2(bits, found);
    if v2 == Outcome::Decoded {
        return v2;
    }
    v3(bits, found).max(v2)
}

fn checksum(msg: &[u8], nibble: usize) -> bool {
    let mut sum: u32 = (0..nibble.saturating_sub(1))
        .step_by(2)
        .map(|k| u32::from(msg[k >> 1] >> 4) + u32::from(msg[k >> 1] & 0x0f))
        .sum();
    let expected = if nibble & 1 == 1 {
        sum += u32::from(msg[nibble >> 1] >> 4);
        u32::from(msg[nibble >> 1] & 0x0f) | u32::from(msg[(nibble + 1) >> 1] & 0xf0)
    } else {
        u32::from(msg[nibble >> 1] >> 4) | u32::from(msg[nibble >> 1] & 0x0f) << 4
    };
    sum & 0xff == expected
}

fn temperature(msg: &[u8]) -> f32 {
    let tenths =
        u32::from(msg[5] >> 4) * 100 + u32::from(msg[4] & 0x0f) * 10 + u32::from(msg[4] >> 4);
    let value = tenths as f32 / 10.0 + f32::from(msg[5] & 0x07) * 100.0;
    if msg[5] & 0x08 != 0 { -value } else { value }
}

fn humidity(msg: &[u8]) -> u8 {
    (msg[6] & 0x0f) * 10 + (msg[6] >> 4)
}

fn bcd_ok(nibbles: &[u8]) -> bool {
    nibbles.iter().all(|&nibble| nibble <= 9)
}

fn base(model: &str, msg: &[u8], bits: usize) -> IsmMessage {
    let device = (msg[2] & 0x0f) | (msg[3] & 0xf0);
    IsmMessage {
        channel: Some((msg[2] >> 4).to_string()),
        battery_ok: Some((msg[3] >> 2) & 1 == 0),
        ..reading(model, device.to_string(), &msg[..bits.div_ceil(8)], bits)
    }
}

fn v2_sync(bits: &Bits) -> Option<usize> {
    let b = bits.row(0);
    let test = u32::from_be_bytes([b[3], b[4], b[5], b[6]]);
    (0..8).find(|&shift| {
        let mask = 0xffff_0000u32 >> shift;
        test & mask == 0x5599_0000 >> shift || test & mask == 0xaa99_0000 >> shift
    })
}

fn v2(bits: &Bits, found: &mut Found) -> Outcome {
    let b = bits.row(0);
    if !((b[1] == 0x55 && b[2] == 0x55) || (b[1] == 0xaa && b[2] == 0xaa)) {
        return Outcome::Skip;
    }
    let Some(shift) = v2_sync(bits) else {
        return Outcome::Skip;
    };
    let mut packed = Packed::new();
    bits.manchester(0, shift + 40, V2_MAX_BITS, &mut packed);
    let len = packed.len();
    let mut msg = *packed.bytes();
    reflect_nibbles(&mut msg[..len.div_ceil(8)]);
    let sensor = u16::from_be_bytes([msg[0], msg[1]]);
    match sensor {
        THGR122N | THGR968 => {
            let valid = matches!(len, 68 | 76) && checksum(&msg, 15);
            let model = match (sensor, len) {
                (THGR968, _) => "Oregon-THGR968",
                (_, 68) => "Oregon-THGR228N",
                _ => "Oregon-THGR122N",
            };
            Outcome::checked(valid, || {
                Outcome::emit(
                    found,
                    IsmMessage {
                        temperature_c: Some(temperature(&msg)),
                        humidity_pct: Some(humidity(&msg)),
                        ..base(model, &msg, len)
                    },
                )
            })
        }
        THN132N if len == 64 => Outcome::checked(checksum(&msg, 12), || {
            let value = temperature(&msg);
            if !bcd_ok(&[msg[5] >> 4, msg[4] & 0x0f, msg[4] >> 4])
                || !(-50.0..=70.0).contains(&value)
            {
                return Outcome::Skip;
            }
            Outcome::emit(
                found,
                IsmMessage {
                    temperature_c: Some(value),
                    ..base("Oregon-THN132N", &msg, len)
                },
            )
        }),
        _ => Outcome::Skip,
    }
}

fn v3_preamble(b: &[u8]) -> bool {
    (b[0] & 0x0f == 0x0f && b[1] == 0xff && b[2] & 0xc0 == 0xc0)
        || (b[0] & 0x0f == 0 && b[1] == 0 && b[2] & 0xc0 == 0)
}

fn v3(bits: &Bits, found: &mut Found) -> Outcome {
    if !v3_preamble(bits.row(0)) {
        return Outcome::Skip;
    }
    let start = bits.search(0, 0, &V3_PREAMBLE, 16) + 16;
    let len = bits.len(0).saturating_sub(start);
    if !(56..=V3_BYTES * 8).contains(&len) {
        return Outcome::Skip;
    }
    let mut msg = [0u8; V3_BYTES];
    bits.extract(0, start, &mut msg, len);
    reflect_nibbles(&mut msg[..len.div_ceil(8)]);
    let sensor = u16::from_be_bytes([msg[0], msg[1]]);
    if WGR800.contains(&sensor) {
        return Outcome::checked(checksum(&msg, 17), || wgr800(&msg, found));
    }
    if sensor & THGR810_MASK == THGR810 {
        return Outcome::checked(checksum(&msg, 15), || thgr810(&msg, found));
    }
    Outcome::Skip
}

fn wgr800(msg: &[u8], found: &mut Found) -> Outcome {
    if !bcd_ok(&[
        msg[5] & 0x0f,
        msg[6] >> 4,
        msg[6] & 0x0f,
        msg[7] >> 4,
        msg[7] & 0x0f,
        msg[8] >> 4,
    ]) {
        return Outcome::Skip;
    }
    let gust =
        f32::from(msg[5] & 0x0f) / 10.0 + f32::from(msg[6] >> 4) + f32::from(msg[6] & 0x0f) * 10.0;
    let average =
        f32::from(msg[7] >> 4) / 10.0 + f32::from(msg[7] & 0x0f) + f32::from(msg[8] >> 4) * 10.0;
    if gust > 56.0 || average > 56.0 {
        return Outcome::Skip;
    }
    Outcome::emit(
        found,
        IsmMessage {
            wind_max_m_s: Some(gust),
            wind_avg_m_s: Some(average),
            wind_dir_deg: Some(f32::from(msg[4] >> 4) * 22.5),
            ..base("Oregon-WGR800", msg, 76)
        },
    )
}

fn thgr810(msg: &[u8], found: &mut Found) -> Outcome {
    let value = temperature(msg);
    let digits = [
        msg[5] >> 4,
        msg[4] & 0x0f,
        msg[4] >> 4,
        msg[6] & 0x0f,
        msg[6] >> 4,
    ];
    if !bcd_ok(&digits) || !(-50.0..=70.0).contains(&value) {
        return Outcome::Skip;
    }
    Outcome::emit(
        found,
        IsmMessage {
            temperature_c: Some(value),
            humidity_pct: Some(humidity(msg)),
            ..base("Oregon-THGR810", msg, 68)
        },
    )
}

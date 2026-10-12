use sdrmm_dsp::{crc8_msb, lfsr_digest8_reflect};
use sdrmm_wire::{IsmCoding, IsmIntegrity, IsmMessage};

use crate::ism::{
    bits::Bits,
    device::{Device, Found, Outcome, fsk, ook, reading, tenths},
    slicer::Timing,
};

pub(super) const TX141: Device = ook(
    IsmCoding::Pwm,
    Timing::new(208, 417, 1_700).sync(833).gap(625),
    tx141,
)
.integrity(IsmIntegrity::Repeats);

pub(super) const TX29: Device = fsk(Timing::new(55, 55, 4_000), tx29).integrity(IsmIntegrity::Crc);

pub(super) const TX35: Device =
    fsk(Timing::new(105, 105, 4_000), tx35).integrity(IsmIntegrity::Crc);

const IT_PREAMBLE: [u8; 3] = [0xa2, 0xdd, 0x49];
const NO_HUMIDITY: u8 = 0x6a;
const PROBE: u8 = 0x7d;

fn tx141_row(bits: &Bits) -> Option<usize> {
    let repeats = if bits.rows() > 5 { 5 } else { 3 };
    bits.repeated_row(repeats, 32).or_else(|| {
        (bits.rows() <= 4)
            .then(|| {
                (0..bits.rows()).find(|&row| {
                    let b = bits.row(row);
                    matches!(bits.len(row), 40 | 41)
                        && lfsr_digest8_reflect(0x31, 0xf4, &b[..4]) == b[4]
                })
            })
            .flatten()
    })
}

fn tx141(bits: &mut Bits, found: &mut Found) -> Outcome {
    bits.invert();
    let Some(row) = tx141_row(bits) else {
        return Outcome::Skip;
    };
    let len = bits.len(row);
    let humid = match len {
        40 => true,
        41 if bits.rows() <= 12 => true,
        37..=39 => false,
        _ => return Outcome::Skip,
    };
    let b = &bits.row(row)[..5];
    let low = if humid {
        b[1] >> 7 == 1
    } else {
        b[1] >> 7 == 0
    };
    let temperature = tenths((i32::from(b[1] & 0x0f) << 8 | i32::from(b[2])) - 500);
    let humidity = b[3];
    let sane = b[0] != 0
        && (!humid || (1..=100).contains(&humidity))
        && (-40.0..=140.0).contains(&temperature);
    if !sane {
        return Outcome::Skip;
    }
    if humid && lfsr_digest8_reflect(0x31, 0xf4, &b[..4]) != b[4] {
        return Outcome::Mic;
    }
    let (model, bytes, integrity) = if humid {
        ("LaCrosse-TX141THBv2", 5, IsmIntegrity::Crc)
    } else {
        ("LaCrosse-TX141Bv2", 4, IsmIntegrity::Repeats)
    };
    Outcome::emit(
        found,
        IsmMessage {
            channel: Some(((b[1] & 0x30) >> 4).to_string()),
            battery_ok: Some(!low),
            temperature_c: Some(temperature),
            humidity_pct: humid.then_some(humidity),
            integrity,
            ..reading(model, b[0].to_string(), &b[..bytes], len)
        },
    )
}

fn tx29(bits: &mut Bits, found: &mut Found) -> Outcome {
    lacrosse_it(bits, found, "LaCrosse-TX29IT")
}

fn tx35(bits: &mut Bits, found: &mut Found) -> Outcome {
    lacrosse_it(bits, found, "LaCrosse-TX35DTHIT")
}

fn lacrosse_it(bits: &Bits, found: &mut Found, model: &str) -> Outcome {
    Outcome::best((0..bits.rows()).map(|row| {
        let start = bits.search(row, 0, &IT_PREAMBLE, 24);
        if start >= bits.len(row) {
            return Outcome::Skip;
        }
        let mut b = [0u8; 5];
        bits.extract(row, start + 20, &mut b, 40);
        Outcome::checked(crc8_msb(0x31, 0, &b[..4]) == b[4], || {
            let mut id = u32::from(b[0] & 0x0f) << 2 | u32::from(b[1] >> 6);
            let tens = f32::from(b[1] & 0x0f) * 10.0;
            let temperature = tens + f32::from(b[2] >> 4) + f32::from(b[2] & 0x0f) * 0.1 - 40.0;
            let humidity = b[3] & 0x7f;
            if humidity == PROBE {
                id += 0x40;
            }
            Outcome::emit(
                found,
                IsmMessage {
                    battery_ok: Some(b[3] >> 7 == 0),
                    temperature_c: Some(temperature),
                    humidity_pct: (humidity != NO_HUMIDITY && humidity != PROBE)
                        .then_some(humidity),
                    ..reading(model, id.to_string(), &b, 40)
                },
            )
        })
    }))
}

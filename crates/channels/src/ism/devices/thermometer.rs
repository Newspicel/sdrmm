use sdrmm_dsp::{lfsr_digest8, lfsr_digest8_reflect};
use sdrmm_wire::{IsmCoding, IsmIntegrity, IsmMessage};

use crate::ism::{
    bits::Bits,
    device::{Device, Found, Outcome, fahrenheit, ook, reading, tenths},
    slicer::Timing,
};

pub(super) const F007TH: Device =
    ook(IsmCoding::Manchester, Timing::new(500, 0, 2_400), f007th).integrity(IsmIntegrity::Crc);

pub(super) const TP12: Device = ook(
    IsmCoding::Ppm,
    Timing::new(500, 1_500, 4_000).gap(2_000),
    tp12,
)
.integrity(IsmIntegrity::Crc);

const F007TH_PREAMBLES: [([u8; 2], usize); 2] = [([0x01, 0x45], 16), ([0xfd, 0x45], 15)];

fn f007th(bits: &mut Bits, found: &mut Found) -> Outcome {
    let mut outcome = Outcome::Skip;
    for row in 0..bits.rows() {
        for (pattern, step) in F007TH_PREAMBLES {
            let mut at = 0;
            loop {
                at = bits.search(row, at, &pattern, 12);
                if at + 8 + 48 > bits.len(row) {
                    break;
                }
                let mut b = [0u8; 6];
                bits.extract(row, at + 8, &mut b, 48);
                outcome = outcome.max(f007th_message(&b, found));
                if outcome == Outcome::Decoded {
                    return outcome;
                }
                at += step;
            }
        }
    }
    outcome
}

fn f007th_message(b: &[u8; 6], found: &mut Found) -> Outcome {
    if lfsr_digest8(0x98, 0x3e, &b[..5]) ^ 0x64 != b[5] {
        return Outcome::Mic;
    }
    let raw = i32::from(b[2] & 0x0f) << 8 | i32::from(b[3]);
    let temperature = tenths(raw - 400);
    if b[4] > 100 || !(-40.0..344.0).contains(&temperature) {
        return Outcome::Skip;
    }
    Outcome::emit(
        found,
        IsmMessage {
            channel: Some((((b[2] & 0x70) >> 4) + 1).to_string()),
            battery_ok: Some(b[2] & 0x80 == 0),
            temperature_c: Some(fahrenheit(temperature)),
            humidity_pct: Some(b[4]),
            ..reading("Ambientweather-F007TH", b[1].to_string(), b, 48)
        },
    )
}

fn tp12(bits: &mut Bits, found: &mut Found) -> Outcome {
    let repeats = if bits.rows() > 5 { 5 } else { 2 };
    let Some(row) = bits.repeated_prefix(repeats, 40) else {
        return Outcome::Skip;
    };
    let b = &bits.row(row)[..5];
    if b[..4].iter().all(|&byte| byte == 0) || bits.len(row) != 41 {
        return Outcome::Skip;
    }
    Outcome::checked(lfsr_digest8_reflect(0x51, 0x04, &b[..4]) == b[4], || {
        let first = i32::from(b[2] & 0xf0) << 4 | i32::from(b[1]);
        let second = i32::from(b[2] & 0x0f) << 8 | i32::from(b[3]);
        Outcome::emit(
            found,
            IsmMessage {
                temperature_c: Some(tenths(first - 200)),
                temperature_2_c: Some(tenths(second - 200)),
                ..reading("Thermopro-TP12", b[0].to_string(), b, 41)
            },
        )
    })
}

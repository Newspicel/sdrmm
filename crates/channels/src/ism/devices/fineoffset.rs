use sdrmm_dsp::crc8_msb;
use sdrmm_wire::{IsmCoding, IsmIntegrity, IsmMessage};

use crate::ism::{
    bits::{Bits, add_bytes},
    device::{Device, Found, Outcome, fsk, ook, reading, tenths},
    slicer::Timing,
};

pub(super) const WH2: Device = ook(
    IsmCoding::Pwm,
    Timing::new(500, 1_500, 1_200).tolerance(160),
    wh2,
)
.integrity(IsmIntegrity::Crc);

pub(super) const WHX080: Device =
    ook(IsmCoding::Pwm, Timing::new(544, 1_524, 2_800), whx080).integrity(IsmIntegrity::Crc);

pub(super) const WH24: Device = fsk(Timing::new(58, 58, 20_000), wh24).integrity(IsmIntegrity::Crc);

const WIND_DEGREES: [u16; 16] = [
    0, 23, 45, 68, 90, 113, 135, 158, 180, 203, 225, 248, 270, 293, 315, 338,
];
const SYNC: [u8; 3] = [0xaa, 0x2d, 0xd4];
const UVI_UPPER: [u16; 13] = [
    432, 851, 1210, 1570, 2017, 2450, 2761, 3100, 3512, 3918, 4277, 4650, 5029,
];

fn wh2(bits: &mut Bits, found: &mut Found) -> Outcome {
    let head = bits.row(0);
    let (model, start, offset) = match (bits.len(0), head[0]) {
        (48, 0xff) => ("Fineoffset-WH2", 8, false),
        (47, 0xfe) => ("Fineoffset-WH5", 7, true),
        (49, 0xff) if head[1] & 0x80 != 0 => ("Fineoffset-TelldusProove", 9, false),
        _ => return Outcome::Skip,
    };
    let mut b = [0u8; 5];
    bits.extract(0, start, &mut b, 40);
    if crc8_msb(0x31, 0, &b[..4]) != b[4] {
        return Outcome::Mic;
    }
    if b[0] >> 4 != 4 {
        return Outcome::Skip;
    }
    let id = (b[0] & 0x0f) << 4 | b[1] >> 4;
    let raw = i32::from(b[1] & 0x0f) << 8 | i32::from(b[2]);
    let temperature = if offset {
        tenths(raw - 400)
    } else if raw & 0x800 != 0 {
        tenths(-(raw & 0x7ff))
    } else {
        tenths(raw)
    };
    if offset && !(-40.0..=60.0).contains(&temperature) {
        return Outcome::Skip;
    }
    Outcome::emit(
        found,
        IsmMessage {
            temperature_c: Some(temperature),
            humidity_pct: (b[3] != 0xff).then_some(b[3]),
            ..reading(model, id.to_string(), &b, 40)
        },
    )
}

fn whx080(bits: &mut Bits, found: &mut Found) -> Outcome {
    if bits.rows() != 1 {
        return Outcome::Skip;
    }
    let mut br = [0u8; 11];
    match bits.len(0) {
        88..=99 => br.copy_from_slice(&bits.row(0)[..11]),
        87 => {
            bits.extract(0, 7, &mut br[1..], 80);
            br[0] = bits.row(0)[0] >> 1 | 0x80;
        }
        _ => return Outcome::Skip,
    }
    if br[0] != 0xff {
        return Outcome::Skip;
    }
    if crc8_msb(0x31, 0xff, &br) != 0 {
        return Outcome::Mic;
    }
    if br[1] >> 4 != 0x0a {
        return Outcome::Skip;
    }
    let raw = i32::from(br[2] & 0x03) << 8 | i32::from(br[3]);
    let rain = u32::from(br[7] & 0x0f) << 8 | u32::from(br[8]);
    let id = (br[1] << 4) | (br[2] >> 4);
    Outcome::emit(
        found,
        IsmMessage {
            battery_ok: Some(br[9] >> 4 != 1),
            temperature_c: Some(tenths(raw - 400)),
            humidity_pct: Some(br[4]),
            wind_dir_deg: Some(f32::from(WIND_DEGREES[usize::from(br[9] & 0x0f)])),
            wind_avg_m_s: Some(f32::from(br[5]) * 0.34),
            wind_max_m_s: Some(f32::from(br[6]) * 0.34),
            rain_mm: Some(rain as f32 * 0.3),
            ..reading("Fineoffset-WHx080", id.to_string(), &br, 88)
        },
    )
}

fn wh24(bits: &mut Bits, found: &mut Found) -> Outcome {
    let len = bits.len(0);
    if !(190..=215).contains(&len) {
        return Outcome::Skip;
    }
    let offset = bits.search(0, 0, &SYNC, 24) + 24;
    if offset + 17 * 8 > len {
        return Outcome::Skip;
    }
    let wh24 = len - offset - 17 * 8 < 8 && offset < 61;
    let mut b = [0u8; 17];
    bits.extract(0, offset, &mut b, 17 * 8);
    if b[0] != 0x24 {
        return Outcome::Skip;
    }
    if crc8_msb(0x31, 0, &b[..16]) != 0 || add_bytes(&b[..16]) & 0xff != u32::from(b[16]) {
        return Outcome::Mic;
    }
    let (model, factor, cup) = if wh24 {
        ("Fineoffset-WH24", 1.12, 0.3)
    } else {
        ("Fineoffset-WH65B", 0.51, 0.254)
    };
    Outcome::emit(found, wh24_reading(&b, model, factor, cup))
}

fn wh24_reading(b: &[u8; 17], model: &str, factor: f32, cup: f32) -> IsmMessage {
    let direction = u16::from(b[2]) | u16::from(b[3] & 0x80) << 1;
    let raw = i32::from(b[3] & 0x07) << 8 | i32::from(b[4]);
    let wind = u16::from(b[6]) | u16::from(b[3] & 0x10) << 4;
    let rain = u32::from(b[8]) << 8 | u32::from(b[9]);
    let uv = u16::from(b[10]) << 8 | u16::from(b[11]);
    let light = u32::from(b[12]) << 16 | u32::from(b[13]) << 8 | u32::from(b[14]);
    let uvi = UVI_UPPER.iter().take_while(|&&upper| upper < uv).count();
    IsmMessage {
        battery_ok: Some(b[3] & 0x08 == 0),
        temperature_c: (raw != 0x7ff).then(|| tenths(raw - 400)),
        humidity_pct: (b[5] != 0xff).then_some(b[5]),
        wind_dir_deg: (direction != 0x1ff).then_some(f32::from(direction)),
        wind_avg_m_s: (wind != 0x1ff).then(|| f32::from(wind) * 0.125 * factor),
        wind_max_m_s: (b[7] != 0xff).then(|| f32::from(b[7]) * factor),
        rain_mm: Some(rain as f32 * cup),
        uv_index: (uv != 0xffff).then_some(uvi as f32),
        light_lux: (light != 0xff_ffff).then_some(light as f32 * 0.1),
        ..reading(model, b[1].to_string(), b, 17 * 8)
    }
}

pub(super) const WH31E: Device =
    fsk(Timing::new(56, 56, 1_500).gap(1_800), wh31e).integrity(IsmIntegrity::Crc);

const WH31E_KIND: u8 = 0x30;
const WH31B_KIND: u8 = 0x37;
const WH40_KIND: u8 = 0x40;

fn ecowitt_valid(b: &[u8], len: usize) -> bool {
    crc8_msb(0x31, 0, &b[..len]) == 0 && add_bytes(&b[..len]) & 0xff == u32::from(b[len])
}

fn wh31e(bits: &mut Bits, found: &mut Found) -> Outcome {
    Outcome::best((0..bits.rows()).map(|row| {
        let start = bits.search(row, 0, &SYNC, 24);
        if start == bits.len(row) {
            return Outcome::Skip;
        }
        let mut b = [0u8; 18];
        bits.extract(row, start + 24, &mut b, 18 * 8);
        match b[0] {
            WH31E_KIND | WH31B_KIND => Outcome::checked(ecowitt_valid(&b, 6), || {
                let raw = i32::from(b[2] & 0x03) << 8 | i32::from(b[3]);
                let model = if b[0] == WH31E_KIND {
                    "AmbientWeather-WH31E"
                } else {
                    "AmbientWeather-WH31B"
                };
                Outcome::emit(
                    found,
                    IsmMessage {
                        channel: Some((((b[2] & 0x70) >> 4) + 1).to_string()),
                        battery_ok: Some(b[2] & 0x04 == 0),
                        temperature_c: Some(tenths(raw - 400)),
                        humidity_pct: Some(b[4]),
                        ..reading(model, b[1].to_string(), &b[..7], 56)
                    },
                )
            }),
            WH40_KIND => Outcome::checked(ecowitt_valid(&b, 8), || {
                let id = u32::from(b[1] & 0x0f) << 16 | u32::from(b[2]) << 8 | u32::from(b[3]);
                let rain = u16::from_be_bytes([b[5], b[6]]);
                Outcome::emit(
                    found,
                    IsmMessage {
                        rain_mm: Some(f32::from(rain) * 0.1),
                        ..reading("EcoWitt-WH40", id.to_string(), &b[..9], 72)
                    },
                )
            }),
            _ => Outcome::Skip,
        }
    }))
}

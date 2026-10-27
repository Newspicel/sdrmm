use sdrmm_dsp::lfsr_digest8;
use sdrmm_wire::{IsmCoding, IsmMessage};

use crate::ism::{
    bits::{Bits, add_bytes, parity},
    device::{Device, Found, Outcome, fahrenheit, ook, reading, signed, tenths},
    slicer::Timing,
};

pub(super) const TXR: Device = ook(
    IsmCoding::Pwm,
    Timing::new(220, 408, 4_000).sync(620).gap(500),
    txr,
);

pub(super) const TX606: Device = ook(
    IsmCoding::Ppm,
    Timing::new(2_000, 4_000, 10_000).gap(7_000),
    tx606,
);

pub(super) const TXC609: Device = ook(
    IsmCoding::Ppm,
    Timing::new(1_000, 2_000, 10_000).gap(3_000),
    txc609,
);

const TOWER: u8 = 0x04;
const TOWER_BYTES: usize = 7;
const WIND_RAIN: u8 = 0x31;
const WIND_CLIMATE: u8 = 0x38;
const FIVE_IN_ONE_BYTES: usize = 8;
const CHANNELS: [char; 4] = ['C', 'E', 'B', 'A'];
const NO_HUMIDITY: u8 = 127;
const WIND_SECTORS: [u8; 16] = [14, 11, 13, 12, 15, 10, 0, 9, 3, 6, 4, 5, 2, 7, 1, 8];
const MM_PER_INCH: f32 = 25.4;
const KMH_PER_MS: f32 = 3.6;

fn txr(bits: &mut Bits, found: &mut Found) -> Outcome {
    bits.invert();
    Outcome::best((0..bits.rows()).map(|row| txr_row(bits, row, found)))
}

fn txr_row(bits: &Bits, row: usize, found: &mut Found) -> Outcome {
    let len = bits.len(row) / 8;
    if !(6..=10).contains(&len) {
        return Outcome::Skip;
    }
    let b = &bits.row(row)[..len];
    if b[0] == 0 && b[1] == 0 && b[2] == 0 && b[len - 1] == 0 {
        return Outcome::Skip;
    }
    let expected = match b[2] & 0x3f {
        TOWER => TOWER_BYTES,
        WIND_RAIN | WIND_CLIMATE => FIVE_IN_ONE_BYTES,
        _ => return Outcome::Skip,
    };
    if len < expected {
        return Outcome::Skip;
    }
    let b = &b[..expected];
    let valid = add_bytes(&b[..expected - 1]) & 0xff == u32::from(b[expected - 1])
        && !parity(&b[2..expected - 1]);
    if !valid {
        return Outcome::Mic;
    }
    let channel = CHANNELS[usize::from(b[0] >> 6)];
    if channel == 'E' {
        return Outcome::Skip;
    }
    let message = if expected == TOWER_BYTES {
        tower(b)
    } else {
        five_in_one(b)
    };
    message.map_or(Outcome::Skip, |message| {
        Outcome::emit(
            found,
            IsmMessage {
                channel: Some(channel.to_string()),
                battery_ok: Some(b[2] & 0x40 != 0),
                ..message
            },
        )
    })
}

fn tower(b: &[u8]) -> Option<IsmMessage> {
    let id = u32::from(b[0] & 0x3f) << 8 | u32::from(b[1]);
    let humidity = b[3] & 0x7f;
    if humidity > 100 && humidity != NO_HUMIDITY {
        return None;
    }
    let raw = (u32::from(b[4] & 0x7f) << 7 | u32::from(b[5] & 0x7f)) as i32;
    let temperature = tenths(raw - 1_000);
    (-40.0..=70.0).contains(&temperature).then(|| IsmMessage {
        temperature_c: Some(temperature),
        humidity_pct: (humidity != NO_HUMIDITY).then_some(humidity),
        ..reading("Acurite-Tower", id.to_string(), b, b.len() * 8)
    })
}

fn five_in_one(b: &[u8]) -> Option<IsmMessage> {
    let id = u32::from(b[0] & 0x0f) << 8 | u32::from(b[1]);
    let wind_raw = u32::from(b[3] & 0x1f) << 3 | u32::from(b[4] & 0x70) >> 4;
    let wind_kmh = if wind_raw > 0 {
        wind_raw as f32 * 0.8278 + 1.0
    } else {
        0.0
    };
    let base = IsmMessage {
        wind_avg_m_s: Some(wind_kmh / KMH_PER_MS),
        ..reading("Acurite-5n1", id.to_string(), b, b.len() * 8)
    };
    if b[2] & 0x3f == WIND_RAIN {
        let rain = u32::from(b[5] & 0x7f) << 7 | u32::from(b[6] & 0x7f);
        return Some(IsmMessage {
            wind_dir_deg: Some(f32::from(WIND_SECTORS[usize::from(b[4] & 0x0f)]) * 22.5),
            rain_mm: Some(rain as f32 * 0.01 * MM_PER_INCH),
            ..base
        });
    }
    let raw = (u32::from(b[4] & 0x0f) << 7 | u32::from(b[5] & 0x7f)) as i32;
    let temperature = tenths(raw - 400);
    let humidity = b[6] & 0x7f;
    ((-40.0..=158.0).contains(&temperature) && humidity <= 100).then(|| IsmMessage {
        temperature_c: Some(fahrenheit(temperature)),
        humidity_pct: Some(humidity),
        ..base
    })
}

fn tx606(bits: &mut Bits, found: &mut Found) -> Outcome {
    let Some(row) = bits.repeated_row(3, 32) else {
        return Outcome::Skip;
    };
    if bits.len(row) > 33 {
        return Outcome::Skip;
    }
    let b = &bits.row(row)[..4];
    if b.iter().all(|&byte| byte == 0) {
        return Outcome::Skip;
    }
    Outcome::checked(lfsr_digest8(0x98, 0xf1, &b[..3]) == b[3], || {
        let raw = u32::from(b[1] & 0x0f) << 8 | u32::from(b[2]);
        Outcome::emit(
            found,
            IsmMessage {
                channel: Some((((b[1] & 0x30) >> 4) + 1).to_string()),
                battery_ok: Some(b[1] & 0x80 != 0),
                temperature_c: Some(tenths(signed(raw, 12))),
                ..reading("Acurite-606TX", b[0].to_string(), b, 32)
            },
        )
    })
}

fn txc609(bits: &mut Bits, found: &mut Found) -> Outcome {
    Outcome::best((0..bits.rows()).map(|row| {
        if bits.len(row) != 40 {
            return Outcome::Skip;
        }
        let b = &bits.row(row)[..5];
        let sum = add_bytes(&b[..4]);
        if sum == 0 || sum & 0xff != u32::from(b[4]) {
            return Outcome::Mic;
        }
        let raw = u32::from(b[1] & 0x0f) << 8 | u32::from(b[2]);
        let humidity = b[3];
        if humidity > 100 {
            return Outcome::Skip;
        }
        Outcome::emit(
            found,
            IsmMessage {
                battery_ok: Some(b[1] & 0x80 == 0),
                temperature_c: Some(tenths(signed(raw, 12))),
                humidity_pct: Some(humidity),
                ..reading("Acurite-609TXC", b[0].to_string(), b, 40)
            },
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ism::bits::from_rows;
    use crate::ism::device::tenths_c;

    const TX606_ROW: &str = "00101010100011111100101110111111";

    #[test]
    fn a_606tx_frame_reads_its_signed_twelve_bit_temperature() {
        let rows = [TX606_ROW; 3];
        let mut bits = from_rows(&rows);
        let mut found = Found::new();
        assert_eq!(tx606(&mut bits, &mut found), Outcome::Decoded);
        let [message] = found.as_slice() else {
            panic!("{found:?}");
        };
        assert_eq!(message.model, "Acurite-606TX");
        assert_eq!(message.id, "42");
        assert_eq!(message.channel.as_deref(), Some("1"));
        assert_eq!(message.battery_ok, Some(true));
        assert_eq!(tenths_c(message), Some(-5.3));
        assert_eq!(message.raw, "2a8fcbbf");
        assert_eq!(message.bits, 32);
    }

    #[test]
    fn a_606tx_frame_with_a_broken_digest_reports_a_failed_check() {
        let mut row = TX606_ROW.to_owned();
        row.replace_range(31..32, "0");
        let rows = [row.as_str(); 3];
        let mut bits = from_rows(&rows);
        let mut found = Found::new();
        assert_eq!(tx606(&mut bits, &mut found), Outcome::Mic);
        assert!(found.is_empty());
    }
}

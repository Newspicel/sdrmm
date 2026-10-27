use sdrmm_dsp::crc8_msb;
use sdrmm_wire::{IsmCoding, IsmIntegrity, IsmMessage};

use crate::ism::{
    bits::{Bits, ROW_BYTES, bcd},
    device::{Device, Found, Outcome, ook, reading, signed, tenths},
    slicer::Timing,
};

pub(super) const NEXUS: Device = ook(
    IsmCoding::Ppm,
    Timing::new(1_000, 2_000, 5_000).gap(3_000),
    nexus,
)
.integrity(IsmIntegrity::Repeats)
.fallback();

pub(super) const PROLOGUE: Device = ook(
    IsmCoding::Ppm,
    Timing::new(2_000, 4_000, 10_000).gap(7_000),
    prologue,
)
.integrity(IsmIntegrity::Repeats)
.fallback();

pub(super) const RUBICSON: Device = ook(
    IsmCoding::Ppm,
    Timing::new(1_000, 2_000, 4_800).gap(3_000),
    rubicson,
)
.integrity(IsmIntegrity::Crc);

const NO_HUMIDITY: u8 = 0xcc;

fn rubicson_crc(b: &[u8]) -> bool {
    let packed = [
        b[0],
        b[1],
        b[2],
        b[3] & 0xf0,
        (b[3] & 0x0f) << 4 | b[4] >> 4,
    ];
    crc8_msb(0x31, 0x6c, &packed) == 0
}

fn temperature(b: &[u8]) -> f32 {
    tenths(signed(u32::from(b[1] & 0x0f) << 8 | u32::from(b[2]), 12))
}

fn nexus(bits: &mut Bits, found: &mut Found) -> Outcome {
    let Some(row) = bits.repeated_row(3, 36) else {
        return Outcome::Skip;
    };
    let b = &bits.row(row)[..5];
    if bits.len(row) > 37 || b[3] & 0xf0 != 0xf0 {
        return Outcome::Skip;
    }
    let blank =
        (b[0] == 0 && b[2] == 0 && b[3] == 0) || (b[0] == 0xff && b[2] == 0xff && b[3] == 0xff);
    let humidity = (b[3] & 0x0f) << 4 | b[4] >> 4;
    if blank || b[1] & 0x30 == 0x30 || rubicson_crc(b) || humidity > 100 {
        return Outcome::Skip;
    }
    let model = if humidity == 0 { "Nexus-T" } else { "Nexus-TH" };
    Outcome::emit(
        found,
        IsmMessage {
            channel: Some((((b[1] & 0x30) >> 4) + 1).to_string()),
            battery_ok: Some(b[1] & 0x80 != 0),
            temperature_c: Some(temperature(b)),
            humidity_pct: (humidity != 0).then_some(humidity),
            ..reading(model, b[0].to_string(), b, 36)
        },
    )
}

fn rubicson(bits: &mut Bits, found: &mut Found) -> Outcome {
    let Some(row) = bits.repeated_row(3, 36) else {
        return Outcome::Skip;
    };
    let b = &bits.row(row)[..5];
    if !(36..=38).contains(&bits.len(row)) || b[3] & 0xf0 != 0xf0 {
        return Outcome::Skip;
    }
    Outcome::checked(rubicson_crc(b), || {
        Outcome::emit(
            found,
            IsmMessage {
                channel: Some((((b[1] & 0x30) >> 4) + 1).to_string()),
                battery_ok: Some(b[1] & 0x80 != 0),
                temperature_c: Some(temperature(b)),
                ..reading("Rubicson-Temperature", b[0].to_string(), b, 36)
            },
        )
    })
}

fn prologue(bits: &mut Bits, found: &mut Found) -> Outcome {
    let first = bits.len(0);
    if first > 0 && first <= 8 {
        return Outcome::Skip;
    }
    let Some(row) = bits.repeated_row(4, 36) else {
        return Outcome::Skip;
    };
    let b = &bits.row(row)[..5];
    if bits.len(row) > 37 || !matches!(b[0] & 0xf0, 0x90 | 0x50) {
        return Outcome::Skip;
    }
    let id = (b[0] & 0x0f) << 4 | b[1] >> 4;
    let raw = u32::from(b[2]) << 4 | u32::from(b[3] >> 4);
    let humidity = (b[3] & 0x0f) << 4 | b[4] >> 4;
    Outcome::emit(
        found,
        IsmMessage {
            channel: Some(((b[1] & 0x03) + 1).to_string()),
            battery_ok: Some(b[1] & 0x08 != 0),
            temperature_c: Some(tenths(signed(raw, 12))),
            humidity_pct: (humidity != NO_HUMIDITY && humidity <= 100).then_some(humidity),
            ..reading("Prologue-TH", id.to_string(), b, 36)
        },
    )
}

pub(super) const ALECTO: Device = ook(
    IsmCoding::Ppm,
    Timing::new(2_000, 4_000, 10_000).gap(7_000),
    alecto,
);

fn alecto_checksum(b: &[u8; ROW_BYTES]) -> bool {
    let sum: u32 = b[..4]
        .iter()
        .map(|byte| {
            let reversed = byte.reverse_bits();
            u32::from(reversed & 0x0f) + u32::from(reversed >> 4)
        })
        .sum();
    let sum = if b[1] & 0x7f == 0x6c {
        sum + 7
    } else {
        0x0f_u32.wrapping_sub(sum)
    };
    (((sum & 0x0f) as u8) << 4).reverse_bits() == b[4] >> 4
}

fn alecto(bits: &mut Bits, found: &mut Found) -> Outcome {
    if bits.len(1) != 36 {
        return Outcome::Skip;
    }
    let first = |row: usize| bits.row(row)[0];
    let (one, five) = (bits.row(1), bits.row(5));
    if one[0] != five[0]
        || first(2) != first(6)
        || one[4] & 0x0f != 0
        || five[4] & 0x0f != 0
        || five[0] == 0
        || five[1] == 0
    {
        return Outcome::Skip;
    }
    if !alecto_checksum(one) || !alecto_checksum(five) {
        return Outcome::Mic;
    }
    let b = one;
    let id = b[0].reverse_bits().to_string();
    let base = IsmMessage {
        channel: Some(((b[0] & 0x0c) >> 2).to_string()),
        battery_ok: Some(b[1] & 0x80 == 0),
        ..reading("AlectoV1", id, &b[..5], 36)
    };
    let kind = (b[1] & 0x60) >> 5;
    if kind == 3 && b[1] & 0x0f == 0x0c {
        let rain = u32::from(b[3].reverse_bits()) << 8 | u32::from(b[2].reverse_bits());
        return Outcome::emit(
            found,
            IsmMessage {
                model: "AlectoV1-Rain".to_owned(),
                rain_mm: Some(rain as f32 * 0.25),
                ..base
            },
        );
    }
    let steady = (2..6).all(|row| first(row) == first(row + 1)) && bits.row(3)[4] & 0x0f == 0;
    if kind == 3 || !steady {
        return Outcome::Skip;
    }
    let raw = u32::from(b[2].reverse_bits()) << 4 | u32::from(b[1].reverse_bits() >> 4);
    let humidity = bcd(b[3].reverse_bits()).filter(|&humidity| humidity <= 100);
    humidity.map_or(Outcome::Skip, |humidity| {
        Outcome::emit(
            found,
            IsmMessage {
                model: "AlectoV1-Temperature".to_owned(),
                temperature_c: Some(tenths(signed(raw, 12))),
                humidity_pct: u8::try_from(humidity).ok(),
                ..base
            },
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ism::bits::from_rows;
    use crate::ism::device::{Decode, tenths_c};

    const RUBICSON_ROW: &str = "110001011000111101001110111100000100";
    const NEXUS_T_ROW: &str = "000100001000000011001111111100000000";
    const PROLOGUE_ROW: &str = "100110100011100111111000010101000101";
    const ALECTO_TEMPERATURE_ROW: &str = "010101000000010101110000101000100100";
    const ALECTO_RAIN_ROW: &str = "010101000110110011011110000000000111";
    const ALECTO_SYNC_ROW: &str = "11111111";

    fn only(decode: Decode, rows: &[&str]) -> IsmMessage {
        let mut bits = from_rows(rows);
        let mut found = Found::new();
        assert_eq!(decode(&mut bits, &mut found), Outcome::Decoded);
        assert_eq!(found.len(), 1, "{found:?}");
        found.remove(0)
    }

    fn alecto_rows(frame: &str) -> [&str; 7] {
        [ALECTO_SYNC_ROW, frame, frame, frame, frame, frame, frame]
    }

    #[test]
    fn a_rubicson_frame_passes_its_crc_and_reads_a_negative_temperature() {
        let message = only(rubicson, &[RUBICSON_ROW; 3]);
        assert_eq!(message.model, "Rubicson-Temperature");
        assert_eq!(message.id, "197");
        assert_eq!(message.channel.as_deref(), Some("1"));
        assert_eq!(message.battery_ok, Some(true));
        assert_eq!(tenths_c(&message), Some(-17.8));
        assert_eq!(message.raw, "c58f4ef040");
        assert_eq!(message.bits, 36);
    }

    #[test]
    fn a_rubicson_frame_with_a_broken_crc_reports_a_failed_check() {
        let mut row = RUBICSON_ROW.to_owned();
        row.replace_range(35..36, "1");
        let mut bits = from_rows(&[row.as_str(); 3]);
        let mut found = Found::new();
        assert_eq!(rubicson(&mut bits, &mut found), Outcome::Mic);
        assert!(found.is_empty());
    }

    #[test]
    fn a_nexus_frame_without_humidity_reports_the_temperature_only_model() {
        let message = only(nexus, &[NEXUS_T_ROW; 3]);
        assert_eq!(message.model, "Nexus-T");
        assert_eq!(message.id, "16");
        assert_eq!(message.channel.as_deref(), Some("1"));
        assert_eq!(message.battery_ok, Some(true));
        assert_eq!(tenths_c(&message), Some(20.7));
        assert_eq!(message.humidity_pct, None);
        assert_eq!(message.raw, "1080cff000");
    }

    #[test]
    fn a_prologue_frame_reads_its_nibble_packed_temperature_and_humidity() {
        let message = only(prologue, &[PROLOGUE_ROW; 4]);
        assert_eq!(message.model, "Prologue-TH");
        assert_eq!(message.id, "163");
        assert_eq!(message.channel.as_deref(), Some("2"));
        assert_eq!(message.battery_ok, Some(true));
        assert_eq!(tenths_c(&message), Some(-12.3));
        assert_eq!(message.humidity_pct, Some(69));
        assert_eq!(message.raw, "9a39f85450");
        assert_eq!(message.bits, 36);
    }

    #[test]
    fn an_alecto_temperature_frame_reads_reversed_bytes_and_bcd_humidity() {
        let message = only(alecto, &alecto_rows(ALECTO_TEMPERATURE_ROW));
        assert_eq!(message.model, "AlectoV1-Temperature");
        assert_eq!(message.id, "42");
        assert_eq!(message.channel.as_deref(), Some("1"));
        assert_eq!(message.battery_ok, Some(true));
        assert_eq!(tenths_c(&message), Some(23.4));
        assert_eq!(message.humidity_pct, Some(45));
        assert_eq!(message.raw, "540570a240");
    }

    #[test]
    fn an_alecto_rain_frame_counts_quarter_millimetre_tips() {
        let message = only(alecto, &alecto_rows(ALECTO_RAIN_ROW));
        assert_eq!(message.model, "AlectoV1-Rain");
        assert_eq!(message.id, "42");
        assert_eq!(message.rain_mm, Some(30.75));
        assert_eq!(message.temperature_c, None);
        assert_eq!(message.raw, "546cde0070");
    }

    #[test]
    fn an_alecto_frame_with_a_broken_checksum_reports_a_failed_check() {
        let mut row = ALECTO_TEMPERATURE_ROW.to_owned();
        row.replace_range(35..36, "1");
        let mut bits = from_rows(&alecto_rows(row.as_str()));
        let mut found = Found::new();
        assert_eq!(alecto(&mut bits, &mut found), Outcome::Mic);
        assert!(found.is_empty());
    }
}
